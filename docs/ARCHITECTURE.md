# Architecture

```
┌────────────────────────────── apps ──────────────────────────────┐
│ twitchminer (daemon + ctl)   desktop (Slint)   Android (à venir)      │
└───────────────┬──────────────────────┬───────────────────────────┘
                │ in-process           │ HTTP / WebSocket
                ▼                      ▼
        ┌──────────────┐        ┌──────────────┐
        │   tm-core    │◄───────│  tm-server   │
        │  (moteur)    │        │ (API locale) │
        └──────┬───────┘        └──────────────┘
               │ trait Module
        ┌──────▼───────┐
        │  tm-modules  │  drops · points · predictions · notify · (vos modules)
        └──────┬───────┘
               │
        ┌──────▼───────┐
        │  tm-twitch   │  auth · GQL · PubSub · watcher · manifeste
        └──────────────┘
```

| Crate | Rôle | Dépend de |
|---|---|---|
| `tm-twitch` | Protocole Twitch, sans état ni planification | — |
| `tm-core` | État unique, persistance, comptes, ordonnanceur, système de modules, API de commandes | `tm-twitch` |
| `tm-modules` | Les fonctionnalités, un fichier par module | `tm-core` |
| `tm-server` | Expose le moteur en HTTP + WebSocket | `tm-core` |
| `tm-app` | Démarrage en un appel (moteur + modules + logs + API) pour tous les frontends | tout |
| `tm-android` | Pont JNI (`libtwitchminer_jni.so`) pour un service Android | `tm-app` |
| `tm-client` | Abstraction `Backend` pour les interfaces : moteur local ou daemon distant, même miroir d'état | `tm-core` |
| `tm-ui` | Interface native (Slint), partagée par le bureau et Android : lit `Backend::state()`, agit par `Backend::execute` | `tm-client`, `tm-app` |
| `twitchminer` | Exécutable sans interface + client `ctl` | tout |
| `twitchminer-desktop` | Application de bureau : `tm-ui` + barre système, lancement avec la session, une seule instance ([DESKTOP.md](DESKTOP.md)) | `tm-ui` |

## Une seule source de vérité

Tout l'état de l'application est **une seule valeur** `State` (`crates/core/src/state.rs`) détenue par le `Store`.

- L'état ne change **que** par des `Event` (`crates/core/src/event.rs`).
- `Store::emit(event)` applique l'événement avec le réducteur pur `state::apply`, lui attribue un numéro `seq` croissant, le persiste si besoin et le diffuse.
- Un événement qui ne change rien est ignoré : pas de `seq`, pas de diffusion. Les producteurs peuvent donc émettre sans vérifier.
- Un client se synchronise avec **un snapshot puis les événements**. Il les applique avec le même réducteur s'il est en Rust (`tm_core::state::apply`), ou réimplémente le réducteur, qui est simple.

Conséquence : une interface, un bot Discord, un tableau de bord web ou une app mobile distante se branchent tous de la même façon et voient exactement la même chose.

## Un seul emplacement de données

```
<data>/
  twitchminer.db   SQLite : comptes (+ tokens), réglages, historique, stats journalières, données des modules
  logs/            journaux tournants (14 jours)
```

`<data>` est résolu ainsi :
1. `--data-dir` ;
2. sinon `$TM_DATA_DIR` ;
3. sinon `./data` à côté de l'exécutable si un fichier `portable` y existe (mode portable / clé USB) ;
4. sinon le dossier de données de la plateforme : `%APPDATA%\Thinsh0\TwitchMiner\data` sous Windows, `~/.local/share/twitchminer` sous Linux. Sur Android, l'app passe son `filesDir`.

Tables (`crates/core/src/db.rs`, migrations versionnées par `PRAGMA user_version`) :

| Table | Contenu |
|---|---|
| `accounts` | id Twitch, login, identifiants OAuth (jamais exposés dans `State`) |
| `settings` | un document JSON par portée (`engine`, `module:<id>`, `account:<id>/module:<id>`) |
| `history` | événements notables (points, drops, raids, prédictions, statut…), purge configurable |
| `hourly_stats` | agrégats par heure UTC × compte × chaîne (points, minutes, drops), conservés indéfiniment ; `stats` les regroupe par jour local |
| `kv` | stockage clé-valeur libre, un espace de noms par compte × module ; `ui` = préférences de l'interface graphique (`tm_app::read_ui_prefs` / `write_ui_prefs` : thème, langue, serveur distant), hors de l'état et de l'API |

## Comptes

Chaque compte activé a un `AccountRuntime` (`crates/core/src/account.rs`) :

1. **Validation du token** au démarrage, rafraîchissement automatique si un `refresh_token` existe, statut `auth_required` si le token est révoqué. Dans ce cas, seuls les modules qui n'ont pas besoin de connexion (les notifications) continuent de tourner, pour prévenir qu'il faut se reconnecter. Les erreurs réseau sont réessayées indéfiniment avec backoff.
2. **PubSub** : un pool de WebSockets (50 topics par connexion) qui se reconnecte seul, avec ping/pong et ré-abonnement. Les topics sont comptés par référence entre modules. Après un renouvellement du jeton, les connexions se rouvrent pour réabonner les topics avec le nouveau. Fermé plus de 10 min : notice `realtime_down`.
3. **Ordonnanceur** de visionnage (`scheduler.rs`) : chaque module déclare des `WatchRequest { channel, priority, reason }`. L'ordonnanceur garde les N meilleures chaînes (N = `engine.max_watch_slots`, 2 par défaut : la limite que Twitch crédite) et lance un *watcher* par chaîne.
4. **Supervision des modules** : un module en erreur ou qui panique redémarre avec backoff (5 s → 5 min). Ses demandes de visionnage et ses chaînes sont libérées avant le redémarrage. Un module est démarré ou arrêté à chaud quand son réglage `enabled` change.
5. **Contrôle horaire du token** + réaction immédiate à `ERR_BADAUTH` côté PubSub.

Priorités standard (`tm_core::priority`) : `MANUAL 1000 > DROPS 500 > STREAK 300 > POINTS 100`.

## Le watcher (simulation de visionnage)

`crates/twitch/src/watch.rs`, par chaîne :
- toutes les ~10 s : récupère la playlist HLS de **plus basse qualité** (variante à la plus petite `BANDWIDTH`) et envoie un `HEAD` sur **chaque segment pas encore demandé** (au plus 16 par cycle, mémoire de 256 segments par diffusion). Depuis octobre 2026, Twitch ne crédite les minutes de drop que si chaque segment est demandé, comme le ferait un lecteur. Aucune vidéo n'est téléchargée.
- toutes les 60 s : envoie l'événement spade `minute-watched`, ce qui crédite les points de chaîne et les séries de visionnage.
- toutes les 10 min : revérifie que le stream est toujours en ligne.

## Manifeste Twitch (mises à jour sans recompiler)

Twitch change régulièrement les *hashes* de ses requêtes GQL persistées et ses identifiants client. Tout ce qui est volatil est dans `manifest/twitch.json` :
- une copie est **embarquée** dans le binaire ;
- le moteur vérifie toutes les 6 h `engine.manifest_url` (par défaut le fichier de ce dépôt sur la branche par défaut) et adopte toute révision plus récente et valide, mise en cache dans la base ;
- quand Twitch casse un hash : on corrige `manifest/twitch.json`, on incrémente `revision`, on pousse, et toutes les installations se corrigent seules.

Le schéma est validé (hashes de 64 caractères hexadécimaux, profil par défaut présent…). Un manifeste ne contient que des identifiants, jamais d'URL arbitraire ni de code.

## Robustesse

- **GQL** : lots de 30 requêtes, 6 requêtes concurrentes au maximum par compte, retry avec backoff sur les erreurs transitoires (`service timeout`…). Les erreurs partielles sont tolérées (champ à `null`). `PersistedQueryNotFound` est une erreur distincte, qui signale un manifeste périmé.
- **Réseau** : un seul pool HTTP. TLS par rustls/ring avec les racines Mozilla + les racines du système (fonctionne derrière un proxy d'entreprise). Proxy HTTP configurable, y compris pour les WebSockets (tunnel `CONNECT`).
- **Drops** : si la progression ne bouge pas pendant 8 min, la chaîne est écartée 30 min. Après 2 blocages sur la même campagne, la campagne est écartée 2 h (cas typique : compte de jeu non lié). Un rafraîchissement des campagnes qui échoue est réessayé après 30 s, puis jusqu'à 15 min.
- **Persistance** : SQLite en WAL, toutes les écritures sur un thread dédié, donc dans l'ordre des événements. Checkpoint à l'arrêt, sauvegarde à chaud (`backup`). Sous Unix, le dossier de données est réservé à son propriétaire (`0700`) : la base contient les jetons.
- **Notifications** : envoyées à part, dans l'ordre, sans retenir le moteur. Une limite de débit (429) attend le délai demandé par le service (au plus 1 min), une erreur réseau ou serveur est réessayée ; 3 essais par cible.
- **Sans PubSub** (si Twitch le coupait) : le minage continue grâce aux interrogations périodiques.
  - En ligne / hors ligne : toutes les 5 min.
  - Solde et coffres bonus des chaînes regardées : toutes les 4 min.
  - Progression des drops : toutes les 90 s. Un drop terminé y est aussi récupéré.
  - Inventaire : toutes les heures.

  Seuls les raids, moments et prédictions (purement temps réel) seraient perdus, jusqu'à un passage à Hermes, le successeur de PubSub, déjà joignable (`hermes.twitch.tv`).
- **Une seule instance par compte** : verrou exclusif sur le dossier de données. Entre machines, coordination par l'API avec les `peers` (principal / secondaire, bascule automatique, voir [SERVER.md](SERVER.md)).
- **Jetons** : validation horaire, revalidation immédiate sur une réponse 401 ou un `ERR_BADAUTH` de PubSub (au plus une par minute), contrôle planifié avant l'expiration, rafraîchissement automatique. Une validation impossible (réseau) est réessayée après 30 s, puis jusqu'à 5 min. Sinon le statut passe à `auth_required`.

## Plateformes

- **Linux** : binaire glibc compilé sur une base ancienne (Ubuntu 22.04, glibc 2.35), ou binaire statique musl pour le daemon (Raspberry Pi, serveurs, Steam Deck).
- **Windows** : binaire MSVC.
- **Android** : `tm-core` + `tm-modules` se compilent pour `aarch64-linux-android` (rustls/ring, SQLite embarqué). L'app hôte doit lancer le moteur dans un *foreground service* de type `specialUse`, car le type `dataSync` est limité à 6 h/jour depuis Android 15. Une petite couche Kotlin est nécessaire pour ce service.

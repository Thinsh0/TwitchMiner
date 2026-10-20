# Brief pour l'interface (desktop + Android)

Le backend est terminé et testé. Ce document dit **quoi construire** et **avec quelles données**. Le contrat exact est dans [API.md](API.md).

## Pile recommandée

- **Slint** (Rust natif, rendu GPU/Skia, desktop + Android, langage déclaratif `.slint`). Alternatives : `egui` (plus simple, moins joli) ou `iced`.
- Démarrage : `tm_app::App::start(...)` puis `tm_client::Local::new(app.engine.clone())`.
- Toute l'UI lit `backend.state()` (`watch::Receiver<Arc<State>>`) et agit par `backend.execute(Command)`. **Aucune autre source de données.**
- **Dès le début** : un écran « Se connecter à un serveur » qui crée un `tm_client::Remote` au lieu de `Local`. Le reste de l'UI ne change pas. En mode distant, l'app ne démarre **aucun moteur local**. Pour un secours automatique, voir [SERVER.md](SERVER.md) (rôle `secondary`).
- Si `App::start` échoue avec `EngineError::Lock`, un autre moteur (le daemon) utilise déjà ce dossier : proposer de s'y connecter en distant.

## Organisation

Nouvelle interface, conçue de zéro : **ne pas s'inspirer de l'ancienne app Tauri** (branche `dev`), dont l'organisation est à oublier. Seule la direction artistique peut servir de point de départ (fond très sombre, accent violet, cartes légèrement translucides, thèmes sombre / clair / OLED), sans obligation.

**4 écrans + Réglages.** L'accueil répond à 90 % des questions sans naviguer. Desktop : barre latérale ; mobile : les mêmes 4 écrans en barre du bas (pas d'interface séparée).

Principes :
- **Chaque donnée a un seul écran principal.** Pas de doublon d'un écran à l'autre.
- **Ce qui demande une action de l'utilisateur est toujours au même endroit** : la zone « À faire » en haut de l'accueil. Pas d'alertes éparpillées.
- **Le plus important d'abord, le détail sur demande** (déplier une carte plutôt qu'ouvrir un autre écran).

### 1. Accueil : « Qu'est-ce qui se passe, et dois-je faire quelque chose ? »

De haut en bas :

1. **À faire** (masqué si vide), une carte par action, avec son bouton :
   - compte `auth_required` → « Se reconnecter » (`login_start`) ;
   - entrée du plan `not_linked` d'un jeu de la liste → « Lier mon compte » (ouvre `link_url`) ;
   - entrée `at_risk` / `will_miss` → explication + « Faire en premier » (`module_call drops focus`) ;
   - `logins[]` en cours → le code en très grand + l'URL ;
   - `notices` (mise à jour disponible, doublon entre instances…) → fermeture par `notice_dismiss`.
2. **En ce moment** : les 1 à 2 streams regardés (`accounts[].watching` + `channels`), avec la raison (drops, série, points).
3. **Drop en cours** : jeu, récompense, barre de progression, fin estimée (`view::summary()` ou `Command::Summary`).
4. **Aujourd'hui** : points, minutes, drops (`stats` du jour, ou `session`).
5. **Pause / reprise** en un bouton (`settings_set engine {"paused": bool}`), avec l'état `watching_allowed` (hors plage horaire, en pause, compte en `standby` « miné par <peer> »).

### 2. Drops : « Quoi, dans quel ordre, et pourquoi ? »

- **Le plan** (`module_data.drops.plan`, déjà trié) : une ligne par campagne, avec image (`accounts[].campaigns`), jeu, objectif (récompenses), progression, statut lisible :
  - `active` « En cours », `focus` « En premier », `restricted` « Dès que la chaîne est en direct » ;
  - `planned` « Vers <start_at> », `at_risk` « Juste », `will_miss` « Manquera ~<missing_minutes> », `upcoming` « Commence le <starts_at> » ;
  - `not_linked` « Compte à lier », `skipped` « Mis de côté jusqu'à… », `done` « Terminé ».
- Sur chaque ligne : « Faire en premier » (`focus {campaign_id}`), ou « Annuler » si c'est le focus (`unfocus`).
- **Ma liste de jeux** sur le même écran, à réordonner par glisser-déposer (ordre = priorité) : `settings_set module:drops {"games": [...]}`. Plus les jeux exclus et les récompenses voulues (`wanted_rewards`).
- Actions secondaires : `refresh`, `claim_all`.

### 3. Chaînes : « Où j'en suis côté points ? »

`accounts[].channels` : avatar, en direct ou non, jeu, solde (`points` peut être `null` : afficher « — »), gains de la session. Ajout et retrait : `module_call points add_channel/remove_channel {login}` ; ordre : `settings_set ... {"channels": [...]}`.

### 4. Activité : « Qu'est-ce que ça a rapporté ? »

Historique et statistiques fusionnés : un graphique en haut (`stats` par jour ou par chaîne : points, minutes, drops), la liste des événements en dessous (`history`, paginée par `before_id`, filtrable par type).

### Réglages

- **Comptes** : `accounts[].info`, `.status`, `.modules` (santé) ; `account_set_enabled`, `account_remove`, `login_start`. Avec plusieurs comptes, un sélecteur en haut de l'app filtre les 4 écrans.
- **Formulaires générés depuis les JSON Schema** (`modules_list` → `settings_schema`, `settings_get` → `effective` / `defaults` / `schema`), puis `settings_set` (merge) : booléens, nombres, enums, listes.
- **Avancé, replié par défaut** :
  - serveur distant (`tm_client::Remote`) ;
  - proxy, pairs et rôle (voir SERVER.md) ;
  - **agent IA** : interrupteur `settings_set engine {"agent_enabled": bool}` et le jeton agent (`tm_app::read_token(dir, TokenKind::Agent)`, voir AGENT.md) ;
  - journaux (`app.logs.recent(n)` / `.subscribe()`, ou `GET /api/v1/logs`) ;
  - à propos : `version`, `manifest_revision`, `update_check`, `manifest_refresh`, `backup`.

## Événements utiles pour l'animation et les toasts (`backend.events()`)

- `points_earned` (raison `CLAIM` = coffre, `WATCH_STREAK`, `RAID`…) → petit toast « +50 ».
- `drop_claimed` → toast/notification système.
- `drop_alert` (`at_risk`, `will_miss`, `not_linked` avec `link_url`, `new_campaign`) → notification avec action (ouvrir le lien, « Faire en premier »).
- `account_status_changed{auth_required}` → alerte.
- `watch_started` / `watch_stopped` → transition dans la liste « en cours ».

Ne pas recalculer l'état à partir des événements : le miroir `state()` est déjà à jour. Les événements ne servent qu'à l'effet visuel.

## Plateforme

- **Desktop** : icône dans la barre système (tooltip = `summary`), démarrage avec la session (crate `auto-launch`), fermer la fenêtre = réduire dans la barre. Le moteur continue de tourner.
- **Android** : voir [ANDROID.md](ANDROID.md) (service de premier plan `specialUse`, exemption batterie, guide OneUI « Applications jamais en veille », notification avec progression → Now Bar sur OneUI 8).
- Données : ne rien écrire hors du dossier de `Paths` (la base contient déjà tout).

## Pièges

- `execute` est asynchrone et peut prendre jusqu'à plusieurs secondes (`login_start`, `module_call`) : ne pas bloquer le thread UI.
- `campaigns` peut contenir des dizaines d'entrées : virtualiser les listes.
- `channels[].points` peut être `null` (inconnu) : afficher « — », pas 0.
- `linked: null` = inconnu, pas « non lié ».
- Les identifiants (comptes, chaînes, campagnes) sont des chaînes de caractères, pas des nombres.

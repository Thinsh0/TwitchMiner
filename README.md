# TwitchMiner

Mineur de **drops** et de **points de chaîne** Twitch, en Rust natif : appli de bureau pour Windows et Linux, serveur (Docker, amd64 et arm64) et appli Android pour suivre le tout depuis le téléphone.

Le moteur regarde des streams en arrière-plan, en qualité minimale, pour récupérer les drops et les points. Il tourne dans l'appli de bureau, ou seul sur un petit serveur ; l'appli de bureau et l'appli Android peuvent alors s'y connecter à distance (réseau local ou Tailscale).

## Télécharger

Tout est sur la page des [Releases](https://github.com/Thinsh0/TwitchMiner/releases/latest) :

| Fichier | Pour |
|---|---|
| `TwitchMiner-vX.Y.Z-windows.exe` | **Windows** : l'appli, un seul fichier. La placer dans un dossier où elle restera et la lancer : elle s'ajoute au menu Démarrer et aux applications installées, et se met ensuite à jour seule. |
| `TwitchMiner-vX.Y.Z-android.apk` | **Android** : suivre et piloter un serveur TwitchMiner depuis le téléphone ([docs/ANDROID.md](docs/ANDROID.md)). |
| `ghcr.io/thinsh0/twitchminer` | **Serveur** : image Docker amd64 + arm64 (NAS, Raspberry Pi, mini PC) ([docs/SERVER.md](docs/SERVER.md)). |
| `twitchminer-…-x86_64-unknown-linux-gnu.tar.gz` | **Linux** : appli de bureau + daemon. |
| `twitchminer-…-{x86_64,aarch64}-unknown-linux-musl.tar.gz` | **Linux** : daemon statique, sans aucune dépendance. |
| `twitchminer-…-x86_64-pc-windows-msvc.zip` | **Windows** : appli + daemon en ligne de commande. |
| `twitchminer-…-*-linux-android.tar.gz` | Moteur Android (daemon pour Termux, pont JNI). |

## Démarrer

**Sur un PC** : lancer l'appli, « Utiliser ce PC », puis « Connecter un compte Twitch » : un code s'affiche, à confirmer sur la page Twitch. Ensuite, choisir ses jeux dans **Drops › Ma liste de jeux** (l'ordre fait la priorité) et ses chaînes dans **Chaînes**. Fermer la fenêtre la garde dans la barre système.

**Sur un serveur** (Docker) :

```bash
docker run -d --name twitchminer --restart unless-stopped \
  -v twitchminer:/data -p 7878:7878 ghcr.io/thinsh0/twitchminer
docker logs twitchminer                         # code de connexion Twitch
docker exec twitchminer twitchminer api-token   # jeton pour les applis
```

Puis, dans l'appli de bureau ou Android : « Serveur distant », adresse `http://<serveur>:7878` (toujours `http://` ; avec Tailscale, le nom de la machine) et le jeton. Avec Dockge, Portainer ou Compose, voir [docs/SERVER.md](docs/SERVER.md). Ne jamais ouvrir le port 7878 sur Internet : passer par le réseau local ou Tailscale.

## Fonctionnalités

- **Drops**
  - Découverte des campagnes, même quand Twitch masque le tableau de bord (comptes récents).
  - **Planification** : ton ordre de jeux est respecté, mais une campagne qui finit bientôt passe devant tant que tout le reste tient encore. Une priorité haute n'est jamais sacrifiée pour une plus basse.
  - Les campagnes esport (chaînes imposées) passent en premier dès qu'elles sont en direct.
  - Chaque campagne vise sa meilleure récompense, ou celles que tu choisis.
  - « Faire en premier » sur une campagne, récupération automatique, détection des chaînes ou campagnes bloquées.
  - Progression en direct (pourcentage, temps restant), alertes : campagne à risque, campagne qui sera manquée, compte de jeu à lier, nouvelle campagne.
  - Un jeu de ta liste renommé par Twitch reste reconnu (par son identifiant). Inventaire des récompenses obtenues.
- **Points de chaîne** : chaînes choisies (et, en option, toutes les chaînes suivies), coffres bonus, raids, moments, **séries de visionnage** prioritaires.
- **Prédictions** (optionnel) : 4 stratégies, garde-fous sur les mises (solde, participants, gain min/max, paris discrets), bilan gagné/perdu.
- **Notifications** : Discord, ntfy, Telegram, webhook ; notifications système sur le bureau.
- **Multi-comptes**, chacun isolé. **Historique et statistiques** par jour et par chaîne. **Plages horaires** et pause globale.
- **Plusieurs machines** : un moteur `secondary` laisse le compte au serveur et prend le relais s'il tombe.
- **Résistant aux changements de Twitch** : les identifiants volatils sont dans un manifeste mis à jour à distance, sans recompiler. Le jeton Twitch se renouvelle seul.
- **Agents IA** (Hermes, OpenClaw, Claude…) : serveur MCP en HTTP et stdio, jeton restreint ([docs/AGENT.md](docs/AGENT.md)).
- **Un seul dossier de données** (une base SQLite), proxy HTTP, mode portable, journaux tournants.

## État du projet

| Partie | État |
|---|---|
| Moteur (drops, points, prédictions, notifications, multi-comptes) | ✅ testé contre Twitch |
| Appli de bureau Windows / Linux (Slint) : moteur local ou distant, barre système, mise à jour intégrée (Windows) | ✅ |
| Serveur : image Docker amd64 + arm64, systemd, secours entre machines | ✅ |
| API HTTP / WebSocket, client Rust local ou distant, serveur MCP | ✅ |
| Appli Android (client d'un serveur) | 🆕 première version ; notifications à venir |
| Moteur embarqué dans l'appli Android | 🔜 plus tard |

## En ligne de commande

Le daemon `twitchminer` fait tourner le moteur sans interface et se pilote par son API :

```bash
twitchminer run
#   No account yet. Open https://www.twitch.tv/activate and enter the code: ABCDEFGH
twitchminer ctl status
twitchminer ctl set module:drops '{"games": ["Rust", "World of Warcraft"]}'   # ordre = priorité
twitchminer ctl add-channel xqc
twitchminer ctl call drops plan          # le plan des drops
twitchminer ctl events                   # flux en direct
twitchminer api-token                    # jeton de l'API
```

`twitchminer --help` et `twitchminer ctl --help` listent tout. Page de statut : **http://127.0.0.1:7878/**.

## Documentation

- [Appli de bureau](docs/DESKTOP.md) : moteur local ou distant, Windows, mises à jour
- [Serveur maison + PC + téléphone](docs/SERVER.md) : Docker, Dockge, systemd, Tailscale
- [Android](docs/ANDROID.md) : installer l'APK, se connecter, construire
- [Architecture](docs/ARCHITECTURE.md) : crates, état unique, données, robustesse
- [API](docs/API.md) : le contrat pour les interfaces (commandes, événements, état, réglages)
- [Écrire un module](docs/MODULES.md) : ajouter une fonctionnalité
- [Agent IA](docs/AGENT.md)

## Développement

Rust stable récent (1.91+).

```bash
cargo run -p twitchminer-desktop                 # appli de bureau (TM_UI_DEMO=1 : état fictif)
cargo run -p twitchminer -- run                  # daemon
cargo fmt --all
cargo clippy --workspace --all-targets           # sans avertissement (la CI utilise -D warnings)
cargo test --workspace
packaging/android/build-apk.sh                   # APK (NDK + SDK Android, voir docs/ANDROID.md)
```

Règles d'architecture et conventions : [CLAUDE.md](CLAUDE.md). Tout se fait sur `main` ; la CI construit chaque commit (Windows, Linux gnu/musl/aarch64, Android, APK) et le workflow **Release** publie ces binaires et l'image Docker sans recompiler.

```
crates/twitch    protocole Twitch (auth, GQL, PubSub, visionnage)
crates/core      moteur (état, persistance, comptes, ordonnanceur, modules)
crates/modules   fonctionnalités (drops, points, predictions, notify)
crates/server    API HTTP + WebSocket + MCP
crates/app       démarrage en un appel (daemon, desktop, Android)
crates/client    Backend local / distant pour les interfaces
crates/ui        interface native (Slint), partagée bureau / Android
crates/android   pont JNI (moteur pour une appli Android)
apps/daemon      exécutable `twitchminer`
apps/desktop     appli de bureau `twitchminer-desktop`
apps/android     appli Android (client d'un serveur)
packaging/       Docker, systemd, APK
manifest/        identifiants Twitch mis à jour à distance
integrations/    skill pour agents IA
```

## Plateformes

Windows 10/11 (x86_64), Linux (glibc ≥ 2.35, ou binaire statique musl x86_64 / aarch64), Docker (amd64, arm64), Android 8+ (arm64).

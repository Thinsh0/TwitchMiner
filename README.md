# TwitchMiner

Mineur de **drops** et de **points de chaîne** Twitch, en Rust natif.
Moteur sans interface, modulaire, pilotable par une API locale. Les interfaces (desktop, Android) se branchent dessus.

## Fonctionnalités

- **Drops** : découverte des campagnes, y compris quand Twitch masque le tableau de bord (comptes récents). Choix de la meilleure campagne et chaîne selon vos jeux et votre mode (priorité, fin la plus proche, plus proche de la fin). Suivi en temps réel, récupération automatique. Détection des chaînes ou campagnes bloquées (compte de jeu non lié…).
- **Points de chaîne** : chaînes choisies + chaînes suivies. Coffres bonus, raids, moments, **séries de visionnage** prioritaires. Soldes et multiplicateurs.
- **Prédictions** (optionnel) : 4 stratégies, garde-fous sur les mises.
- **Notifications** : Discord, ntfy, Telegram, webhook.
- **Multi-comptes**, chacun isolé.
- **Historique et statistiques** par jour et par chaîne.
- **Un seul dossier de données** (une base SQLite), un état unique synchronisé en temps réel.
- **Résistant aux changements de Twitch** : les identifiants volatils sont dans un manifeste mis à jour à distance, sans recompiler.
- **Agents IA** (Hermes, OpenClaw, Claude…) : serveur MCP intégré, avec un jeton restreint.
- Proxy HTTP (y compris WebSocket), mode portable, journaux tournants.

## Démarrage rapide

```bash
cargo build --release
./target/release/twitchminer run
#   No account yet. Open https://www.twitch.tv/activate and enter the code: ABCDEFGH

# dans un autre terminal
twitchminer ctl status
twitchminer ctl add-channel xqc
twitchminer ctl set module:drops '{"games": ["Rust", "World of Warcraft"]}'
twitchminer ctl events          # flux en direct
twitchminer ctl history
```

`twitchminer --help` et `twitchminer ctl --help` listent tout.

Une page de statut est aussi servie sur **http://127.0.0.1:7878/**. Elle demande le jeton d'API (`twitchminer api-token`). Elle n'est pas l'interface finale, mais elle suffit pour surveiller un daemon depuis un navigateur ou un téléphone.

## Serveur, NAS, Raspberry Pi

Guide complet : [docs/SERVER.md](docs/SERVER.md). Un seul moteur par dossier de données ; entre machines, une instance `secondary` laisse le compte au serveur et prend le relais s'il tombe.

- **systemd** : `packaging/systemd/twitchminer.service` (service utilisateur).
- **Docker** : `docker build -t twitchminer . && docker run -d -v twitchminer:/data -p 7878:7878 twitchminer`. `docker logs` affiche le code de connexion au premier lancement.
- Le binaire Linux musl (artefact CI) est statique : aucune dépendance.

## Documentation

- [Architecture](docs/ARCHITECTURE.md) : crates, état unique, données, robustesse, plateformes
- [API](docs/API.md) : le contrat pour les interfaces (commandes, événements, état)
- [Écrire un module](docs/MODULES.md)
- [Android et OneUI](docs/ANDROID.md) : service de premier plan, Now Bar, Termux
- [Brief interface](docs/UI_BRIEF.md) : écrans, données et commandes pour l'agent UI
- [Serveur maison + PC + téléphone](docs/SERVER.md) : Tailscale, instance principale / secondaire, bascule automatique
- [Agent IA](docs/AGENT.md) : serveur MCP (HTTP et stdio), jeton agent, skill pour Hermes / OpenClaw

## Structure

```
crates/twitch    protocole Twitch (auth, GQL, PubSub, watcher)
crates/core      moteur (état, persistance, comptes, ordonnanceur, modules)
crates/modules   fonctionnalités (drops, points, predictions, notify)
crates/server    API HTTP + WebSocket
crates/app       démarrage en un appel (daemon, desktop, Android)
crates/client    Backend local/distant pour les interfaces
apps/daemon      exécutable `twitchminer`
manifest/        identifiants Twitch mis à jour à distance
```

## Plateformes

Windows, Linux (glibc ≥ 2.35, ou binaire statique musl) et Android (`aarch64-linux-android`, moteur embarqué dans l'app).

# TwitchMiner

Mineur de **drops** et de **points de chaîne** Twitch, en Rust natif, pour Windows, Linux et Android.

Un moteur sans interface, modulaire, qui tourne en arrière-plan (sur ton PC, ton téléphone ou un petit serveur) et se pilote par une API locale. Les interfaces s'y branchent ; une page de statut web est incluse en attendant l'interface native.

## État du projet

| Partie | État |
|---|---|
| Moteur (drops, points, prédictions, notifications, multi-comptes) | ✅ prêt, testé contre Twitch |
| API HTTP / WebSocket, client Rust local ou distant | ✅ |
| Mode serveur + secours automatique entre machines | ✅ |
| Serveur MCP pour agents IA | ✅ (désactivé par défaut) |
| Interface native desktop + Android (Slint) | 🚧 en cours, voir [docs/UI_BRIEF.md](docs/UI_BRIEF.md) |

## Fonctionnalités

- **Drops**
  - Découverte des campagnes, même quand Twitch masque le tableau de bord (comptes récents).
  - **Planification** : ton ordre de jeux est respecté, mais une campagne qui finit bientôt passe devant tant que tout le reste tient encore. Une priorité haute n'est jamais sacrifiée pour une plus basse.
  - Les campagnes esport (chaînes imposées) passent en premier dès qu'elles sont en direct.
  - Chaque campagne vise sa meilleure récompense, ou celles que tu choisis.
  - « Faire en premier » sur une campagne, récupération automatique, détection des chaînes ou campagnes bloquées.
  - Alertes : campagne à risque, campagne qui sera manquée, compte de jeu à lier, nouvelle campagne.
  - Un jeu de ta liste renommé par Twitch reste reconnu (par son identifiant).
  - Inventaire des récompenses obtenues.
- **Points de chaîne** : chaînes choisies (et, en option, toutes les chaînes suivies), coffres bonus, raids, moments, **séries de visionnage** prioritaires.
- **Prédictions** (optionnel) : 4 stratégies, garde-fous sur les mises (solde, participants, gain min/max, paris discrets), bilan gagné/perdu.
- **Notifications** : Discord, ntfy, Telegram, webhook.
- **Multi-comptes**, chacun isolé. **Historique et statistiques** par jour et par chaîne.
- **Plages horaires** et pause globale.
- **Un seul dossier de données** (une base SQLite) et un état unique synchronisé en temps réel.
- **Résistant aux changements de Twitch** : les identifiants volatils sont dans un manifeste mis à jour à distance, sans recompiler. Le jeton Twitch se renouvelle seul.
- **Agents IA** (Hermes, OpenClaw, Claude…) : serveur MCP en HTTP et stdio, jeton restreint.
- Proxy HTTP (WebSocket compris), mode portable, journaux tournants.

## Démarrage rapide

Binaires : artefacts de la CI (onglet *Actions*) ou des *Releases*. Sinon, depuis les sources (Rust 1.91+) :

```bash
cargo build --release
./target/release/twitchminer run
#   No account yet. Open https://www.twitch.tv/activate and enter the code: ABCDEFGH
```

Dans un autre terminal :

```bash
twitchminer ctl status
twitchminer ctl set module:drops '{"games": ["Rust", "World of Warcraft"]}'   # ordre = priorité
twitchminer ctl add-channel xqc
twitchminer ctl call drops plan          # le plan des drops
twitchminer ctl events                   # flux en direct
twitchminer ctl history
```

`twitchminer --help` et `twitchminer ctl --help` listent tout.

Page de statut : **http://127.0.0.1:7878/** (demande le jeton : `twitchminer api-token`).

## Serveur maison, NAS, Raspberry Pi

Guide complet : [docs/SERVER.md](docs/SERVER.md) (exemple avec Tailscale).

- **systemd** : `packaging/systemd/twitchminer.service`.
- **Docker** : `docker build -t twitchminer . && docker run -d -v twitchminer:/data -p 7878:7878 twitchminer` (`docker logs` affiche le code de connexion).
- Le binaire Linux musl est statique, sans aucune dépendance.
- Un seul moteur par dossier de données. Entre machines, une instance `secondary` laisse le compte au serveur et prend le relais s'il tombe.

## Agent IA

```bash
twitchminer ctl set engine '{"agent_enabled": true}'
twitchminer api-token --agent
```

Puis brancher l'agent sur `http://<ip>:7878/api/v1/mcp` ou sur la commande `twitchminer mcp`. Détails, outils et skill : [docs/AGENT.md](docs/AGENT.md).

## Documentation

- [Architecture](docs/ARCHITECTURE.md) : crates, état unique, données, robustesse, plateformes
- [API](docs/API.md) : le contrat pour les interfaces (commandes, événements, état, réglages, plan des drops)
- [Écrire un module](docs/MODULES.md) : ajouter une fonctionnalité
- [Brief interface](docs/UI_BRIEF.md) : organisation des écrans et données à utiliser
- [Serveur maison + PC + téléphone](docs/SERVER.md)
- [Android et OneUI](docs/ANDROID.md)
- [Agent IA](docs/AGENT.md)

## Développement

```bash
cargo fmt --all
cargo clippy --workspace --all-targets   # sans avertissement (la CI utilise -D warnings)
cargo test --workspace
```

Règles d'architecture et conventions : [CLAUDE.md](CLAUDE.md).

```
crates/twitch    protocole Twitch (auth, GQL, PubSub, visionnage)
crates/core      moteur (état, persistance, comptes, ordonnanceur, modules)
crates/modules   fonctionnalités (drops, points, predictions, notify)
crates/server    API HTTP + WebSocket + MCP
crates/app       démarrage en un appel (daemon, desktop, Android)
crates/client    Backend local / distant pour les interfaces
crates/android   pont JNI
apps/daemon      exécutable `twitchminer`
manifest/        identifiants Twitch mis à jour à distance
integrations/    skill pour agents IA
```

## Plateformes

Windows, Linux (glibc ≥ 2.35, ou binaire statique musl) et Android (`aarch64`, moteur embarqué dans l'app).

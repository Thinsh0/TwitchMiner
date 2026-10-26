# TwitchMiner : notes pour les agents

Rust natif, workspace Cargo. Lire `docs/ARCHITECTURE.md` et `docs/API.md` avant de toucher au code.

## Règles d'architecture (ne pas casser)

- **Une seule source de vérité** : `tm_core::State`. Tout changement passe par `Store::emit(Event)` + le réducteur pur `state::apply`. Ne jamais stocker d'état « vrai » ailleurs.
- **Un seul emplacement de données** : le dossier `Paths` (une base SQLite + `logs/`). Pas de fichiers de config à côté.
- Les interfaces passent par `tm_client::Backend` (ou `Engine::execute` / `Store::snapshot_and_subscribe`). Elles n'appellent jamais `tm-twitch` directement.
- Nouvelle fonctionnalité = nouveau `Module` dans `crates/modules` (voir `docs/MODULES.md`). Le cœur ne connaît aucune fonctionnalité.
- Identifiants Twitch volatils (hashes GQL, client ids) : uniquement dans `manifest/twitch.json`. Incrémenter `revision` à chaque modification.
- Un nouvel `Event` demande : la variante dans `event.rs`, `kind()`, `account()`/`channel_id()` si pertinent, la branche dans `state::apply`, la doc dans `docs/API.md`.
- Une nouvelle `Command` demande : la variante dans `command.rs`, la branche dans `Engine::execute`, la doc dans `docs/API.md`. Si elle est sensible (suppression, fichiers, trafic sortant, secrets), la refuser au jeton agent dans `crates/server/src/access.rs`.
- Interface native : `crates/ui` (Slint) + `apps/desktop`. Textes en anglais dans `@tr()`, traduction française obligatoire dans `crates/ui/translations/fr/LC_MESSAGES/tm-ui.po` (testé). Voir `docs/DESKTOP.md`.
- Outils MCP (agents IA) : `crates/server/src/mcp.rs`, toujours via `run_command` (droits + masquage). Garder `integrations/agent/SKILL.md` et `docs/AGENT.md` à jour.

## Commandes

```bash
cargo fmt --all
cargo clippy --workspace --all-targets      # doit être sans avertissement (CI: -D warnings)
cargo test --workspace
cargo run -p twitchminer -- run             # daemon (données: $TM_DATA_DIR ou dossier plateforme)
cargo run -p twitchminer -- ctl status      # piloter le daemon
cargo run -p twitchminer-desktop            # app de bureau (TM_UI_DEMO=1 : état fictif, voir docs/DESKTOP.md)
TM_TOKEN=... cargo run -p tm-twitch --example smoke   # test réel contre Twitch
```

Avant de valider un changement : `rustup update stable`, puis fmt, clippy et tests. La CI prend toujours la dernière Rust stable, dont les nouvelles règles clippy peuvent la faire échouer alors que tout passe avec une version plus ancienne. Après un push, vérifier que la CI passe.

Android : `cargo ndk --target aarch64-linux-android --platform 26 build -p tm-app`.

Une seule branche : tout se fait sur `main` (la CI ne tourne que là et sur les pull requests).

Publier une version : passer `version` du workspace (`Cargo.toml`) à la nouvelle valeur, `cargo update -w`, valider et pousser sur `main`, attendre que la CI soit verte, puis lancer le workflow **Release** à la main (`workflow_dispatch`, entrée `tag` = `v` + la version). Il ne recompile rien : il reprend les binaires de la CI de ce commit, vérifie que le tag correspond à la version (l'avis de mise à jour compare les deux), crée le tag et la release, y joint les fichiers et publie l'image Docker `ghcr.io/thinsh0/twitchminer` (amd64 + arm64).

## Style

- Commentaires en anglais, sobres ; docs utilisateur (`docs/`, `README.md`) en français.
- Erreurs : `tm_twitch::Error` (couche protocole), `anyhow` dans les modules, `ApiError {code, message}` à la frontière de l'API.
- Pas de `unwrap()` sur des données réseau. Les modules peuvent retourner `Err` : le superviseur les redémarre.

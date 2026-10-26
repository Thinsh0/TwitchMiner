# Développement

Rust natif, un workspace Cargo. Rust stable récent (1.91+). Règles d'architecture et conventions : [CLAUDE.md](../CLAUDE.md) ; à lire avant de toucher au code : [ARCHITECTURE.md](ARCHITECTURE.md) et [API.md](API.md).

## Commandes

```bash
cargo run -p twitchminer-desktop           # appli de bureau
TM_UI_DEMO=1 cargo run -p twitchminer-desktop   # avec un état fictif (aucun compte nécessaire)
cargo run -p twitchminer -- run            # daemon
cargo fmt --all
cargo clippy --workspace --all-targets     # sans avertissement (la CI utilise -D warnings)
cargo test --workspace
packaging/android/build-apk.sh             # APK Android (NDK + SDK, voir ANDROID.md)
```

Captures d'écran sans fenêtre : `TM_UI_DEMO=1 TM_UI_SNAPSHOT=capture.bmp TM_UI_SIZE=412x880 TM_UI_SCREEN=drops` (voir [DESKTOP.md](DESKTOP.md)). Les images du README en viennent (mode démo, noms fictifs).

## Organisation

```
crates/twitch    protocole Twitch (auth, GQL, PubSub, visionnage)
crates/core      moteur (état, persistance, comptes, invités, ordonnanceur, modules)
crates/modules   fonctionnalités (drops, points, predictions, notify)
crates/server    API HTTP + WebSocket + MCP, contrôle d'accès (agent, invités)
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
docs/            documentation (en français)
```

- [Architecture](ARCHITECTURE.md) : crates, état unique, données, robustesse
- [API](API.md) : le contrat pour les interfaces (commandes, événements, état, réglages)
- [Écrire un module](MODULES.md) : ajouter une fonctionnalité
- [Appli de bureau](DESKTOP.md), [Android](ANDROID.md), [Agent IA](AGENT.md)

## CI et versions

Tout se fait sur `main`. La CI construit chaque commit : vérifications (fmt, clippy, tests, doc), binaires Windows, Linux gnu / musl / aarch64-musl, moteur Android et APK.

Publier une version : passer `version` du workspace (`Cargo.toml`) à la nouvelle valeur, `cargo update -w`, pousser sur `main`, attendre que la CI soit verte, puis lancer le workflow **Release** à la main (entrée `tag` = `v` + la version). Il ne recompile rien : il reprend les binaires de la CI de ce commit, crée le tag et la release, y joint les fichiers et publie l'image Docker `ghcr.io/thinsh0/twitchminer` (amd64 + arm64). L'appli Windows propose ensuite la mise à jour d'elle-même.

## Plateformes

Windows 10/11 (x86_64), Linux (glibc ≥ 2.35, ou binaire statique musl x86_64 / aarch64), Docker (amd64, arm64), Android 8+ (arm64).

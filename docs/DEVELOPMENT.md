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
packaging/android/build-apk.sh             # APK Android (NDK + SDK + Gradle, voir ANDROID.md)
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

Tout se fait sur `main`. La CI construit chaque commit : vérifications (fmt, clippy, tests, doc), avis de sécurité des dépendances (`cargo deny check advisories bans sources`, configuré par `deny.toml` : un nouvel avis fait échouer la CI, donc la Release ; mettre à jour la crate avec `cargo update -p <crate>`, ou ajouter l'avis à `ignore` dans `deny.toml` avec la raison s'il n'a pas encore de correctif), binaires Windows, Linux gnu / musl / aarch64-musl, moteur Android et APK. Dependabot propose chaque semaine les mises à jour des crates, des GitHub Actions et des images Docker (`.github/dependabot.yml`).

Publier une version : passer `version` du workspace (`Cargo.toml`) à la nouvelle valeur, `cargo update -w`, pousser sur `main`, attendre que la CI soit verte, puis lancer le workflow **Release** à la main (entrée `tag` = `v` + la version). Il ne recompile rien : il reprend les binaires de la CI de ce commit, crée le tag et la release, y joint les fichiers avec leur signature ed25519 (`<fichier>.sig`) et publie l'image Docker `ghcr.io/thinsh0/twitchminer` (amd64 + arm64). L'appli Windows propose ensuite la mise à jour d'elle-même (installée seulement si la signature est valide). Secrets requis : `RELEASE_SIGNING_KEY` ([DESKTOP.md](DESKTOP.md#signature-des-mises-à-jour-mainteneur)) et ceux de l'APK ([ANDROID.md](ANDROID.md)) ; sans eux, la Release échoue avant de publier quoi que ce soit.

## Signer le manifeste Twitch

`manifest/twitch.json` est téléchargé par toutes les installations (voir [ARCHITECTURE.md](ARCHITECTURE.md#manifeste-twitch-mises-à-jour-sans-recompiler)) et n'est accepté que signé par une clé ed25519 **hors ligne**, différente de `RELEASE_SIGNING_KEY` : elle ne va ni dans le dépôt ni dans les secrets GitHub. Les commandes marchent sous Linux, macOS et, sous Windows, dans **Git Bash** (Git for Windows fournit OpenSSL 3 : `C:\Program Files\Git\usr\bin\openssl.exe`, soit `/usr/bin/openssl` dans Git Bash ; un autre openssl se choisit avec `OPENSSL=/chemin/openssl`).

Une seule fois :

1. Créer la paire, **hors du dépôt** (le script refuse un chemin dans le dépôt ; `*.pem` est ignoré par git) :

   ```bash
   tools/sign-manifest.sh --generate ~/twitchminer-manifest.pem
   ```

   Équivalent à la main (Git Bash compris) :

   ```bash
   openssl genpkey -algorithm ed25519 -out ~/twitchminer-manifest.pem
   # clé publique : les 32 octets bruts, en hexadécimal (64 caractères)
   openssl pkey -in ~/twitchminer-manifest.pem -pubout -outform DER | tail -c 32 | od -An -v -tx1 | tr -d ' \n'; echo
   ```

   Garder la clé privée hors ligne, avec une copie de secours : perdue, les versions qui embarquent sa clé publique ne peuvent plus recevoir de manifeste (il faudra une nouvelle clé et une nouvelle version).
2. Coller la clé publique (64 caractères hexadécimaux) dans `MANIFEST_PUBLIC_KEY` (`crates/twitch/src/manifest.rs`).
3. Signer (étape suivante), commiter `manifest/twitch.json.sig` avec la constante, publier une version : seules les versions qui embarquent la clé acceptent les manifestes téléchargés.

**À chaque modification de `manifest/twitch.json`** :

1. incrémenter `revision` (sinon les installations ne la prennent pas) ;
2. re-signer :

   ```bash
   tools/sign-manifest.sh ~/twitchminer-manifest.pem
   ```

   Le script refuse un fichier en fins de ligne Windows (CRLF : les octets signés ne seraient pas ceux publiés ; `.gitattributes` empêche git de les convertir) et une clé qui n'est pas celle de `MANIFEST_PUBLIC_KEY`. Il écrit `manifest/twitch.json.sig` (64 octets bruts, `openssl pkeyutl -sign -rawin`), le relit et le vérifie avec la clé publique dérivée, puis affiche la clé publique ;
3. commiter et pousser `manifest/twitch.json` **et** `manifest/twitch.json.sig` dans le même commit. Un `.sig` oublié ou périmé fait échouer `cargo test` (donc la CI) dès que la clé est renseignée, et les installations refuseraient le fichier.

Juste après un push, le cache de `raw.githubusercontent.com` (quelques minutes) peut servir un fichier et une signature de versions différentes : le moteur refuse alors le fichier et réessaie 6 h plus tard.

## Plateformes

Windows 10/11 (x86_64), Linux (glibc ≥ 2.35, ou binaire statique musl x86_64 / aarch64), Docker (amd64, arm64), Android 8+ (arm64).

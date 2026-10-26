# Android (et OneUI)

## L'appli (client d'un serveur)

`TwitchMiner-vX.Y.Z-android.apk` (joint à chaque release) suit et pilote un TwitchMiner qui tourne ailleurs : serveur maison (image Docker), PC. Aucun moteur ne tourne sur le téléphone : rien n'est miné par lui, et la batterie n'est pas sollicitée quand l'appli est fermée.

C'est la même interface que l'app de bureau (`crates/ui`), en disposition téléphone : navigation en bas, cartes empilées, réglages sur une colonne. Tout y est : accueil et « À faire », plan des drops, chaînes, activité, réglages du moteur, connexion d'un compte Twitch (le lien d'activation s'ouvre dans le navigateur du téléphone).

**Installer**
1. Télécharger l'APK depuis la page de la release, sur le téléphone.
2. L'ouvrir et autoriser l'installation depuis cette source (Android le demande la première fois).
3. Au premier lancement, saisir l'adresse du serveur et son jeton d'API :
   - adresse : `http://<nom-ou-ip>:7878`, par exemple `http://mon-serveur:7878` avec Tailscale (MagicDNS) ou `http://192.168.1.10:7878` sur le réseau local. Toujours `http://` : le moteur ne parle pas HTTPS, et Tailscale chiffre déjà la connexion ;
   - jeton : `twitchminer api-token` sur le serveur (Docker : `docker exec <conteneur> twitchminer api-token`).

Le téléphone doit pouvoir joindre le serveur : même réseau, ou Tailscale connecté sur les deux. Ne jamais ouvrir le port 7878 sur Internet. Si la connexion échoue, l'écran dit pourquoi (adresse en `https://`, jeton refusé, rien n'écoute, nom inconnu, pas de réponse).

**Mettre à jour** : installer l'APK de la nouvelle release par-dessus (les réglages sont gardés).

**Signature** : la CI signe l'APK avec la clé du dépôt si les secrets `ANDROID_KEYSTORE` (keystore en base64), `ANDROID_KEYSTORE_PASSWORD` et `ANDROID_KEY_ALIAS` existent, sinon avec la clé de test publique `packaging/android/debug.keystore` (n'importe qui peut signer avec : suffisant pour un usage perso, pas pour diffuser). Passer d'une clé à l'autre oblige à désinstaller l'appli une fois. Pour créer une clé privée :

```bash
keytool -genkeypair -keystore release.keystore -alias twitchminer -keyalg RSA -keysize 2048 -validity 10950
base64 -w0 release.keystore   # valeur du secret ANDROID_KEYSTORE
```

**Construire** (sans Gradle : cargo-ndk, puis aapt2, zipalign et apksigner du SDK) :

```bash
export ANDROID_NDK_HOME=... ANDROID_HOME=...      # NDK r27, SDK avec build-tools et une plateforme
packaging/android/build-apk.sh target/TwitchMiner.apk   # ABIS="arm64-v8a x86_64" pour l'émulateur
```

Le code : `apps/android` (bibliothèque `libtwitchminer.so` lancée par la `NativeActivity` d'Android, sans Java ; liens ouverts par un `Intent` via JNI), `packaging/android` (manifeste, icônes, script). Journaux : `adb logcat -s TwitchMiner`.

**Notifications sur le téléphone** : c'est le serveur qui les envoie, par **ntfy**. L'appli ntfy (Play Store, serveur `ntfy.sh`) les reçoit par le push d'Android (Firebase), même téléphone en veille et appli TwitchMiner fermée, sans user la batterie. Pour les activer :

1. dans ntfy, s'abonner à un sujet difficile à deviner (qui connaît le sujet peut lire les messages) ;
2. dans TwitchMiner, **Réglages › Notifications › Options › Destinations** : onglet ntfy, coller le même sujet (ou « Générer » puis le recopier dans ntfy), **Ajouter** ;
3. **Envoyer une notification de test**, puis choisir les **événements envoyés**.

Discord, Telegram et un webhook s'ajoutent au même endroit.

## Moteur sur le téléphone (plus tard)

Le moteur compile aussi pour Android (CI : `aarch64-linux-android`, `x86_64-linux-android`). Le faire tourner sur le téléphone demanderait ce qui suit.

### Architecture recommandée

```
APK
├── libtwitchminer_jni.so  (Rust : tm-app + UI native, ex. Slint via android-activity)
│     └── tm_app::App::start(AppOptions { data_dir: Some(filesDir), api: None, .. })
└── MinerService.kt    (~100 lignes : foreground service + notification)
```

- **Un seul processus** : l'UI Rust et le moteur partagent `Engine`, sans passer par le réseau. L'API HTTP reste optionnelle, utile pour piloter le téléphone depuis le PC.
- `data_dir` = `context.filesDir` (récupéré via JNI ou passé par le service). Tout y est : base SQLite et journaux.
- Le daemon `twitchminer` (artefact CI Android) tourne aussi tel quel dans **Termux**, si on veut zéro APK.

### Rester vivant en arrière-plan

1. **Foreground service** obligatoire, sinon Android tue le minage en quelques minutes.
   - Type `specialUse` (`android:foregroundServiceType="specialUse"` + `<property android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE" android:value="Background media watch for Twitch rewards"/>`).
   - Ne pas utiliser `dataSync` : depuis Android 15, il est limité à 6 h par 24 h.
   - Permissions : `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_SPECIAL_USE`, `POST_NOTIFICATIONS`, `INTERNET`, `WAKE_LOCK`.
2. **Exemption d'optimisation batterie** : `ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`.
3. **OneUI** : en plus, guider l'utilisateur vers *Paramètres → Batterie → Limites d'utilisation en arrière-plan → Applications jamais en veille*. Sans ça, OneUI met l'app en veille malgré tout. Deep link possible : `Intent("com.samsung.android.sm.ACTION_BATTERY")`, avec repli sur les réglages de l'app.
4. Wake lock partiel uniquement pendant le minage. Le moteur est très peu actif : un cycle réseau toutes les ~10 s par chaîne regardée (quelques requêtes `HEAD` légères).

### Notification : la Now Bar de OneUI

Sur Android 16 (OneUI 8+), une notification en cours qui affiche une **progression** peut être promue en **Live Update**. OneUI l'affiche alors dans la **Now Bar** de l'écran verrouillé.

- `Notification.ProgressStyle` (API 36) + `setRequestPromotedOngoing(true)` + permission `POST_PROMOTED_NOTIFICATIONS`.
- Données : `Command::Summary` / `tm_core::view::summary()` → `accounts[].drop` fournit `game`, `drop`, `progress` (0..1), `current_minutes/required_minutes`, `eta`, `image_url`.
- Sans drop en cours : texte « Points : +N cette session » et chaînes regardées (`accounts[].watching`).
- Mettre à jour la notification sur les événements `drop_progress`, `watch_started/stopped` et `drop_claimed`, au plus une fois toutes les 30 s.
- Sur Android < 16 : notification classique avec `setProgress`.

Autres intégrations possibles, toutes alimentées par `summary` :
- **Tuile de réglages rapides** (`TileService`) : pause/reprise, via `account_set_enabled` ;
- **Widget** d'écran d'accueil.

### Pont JNI (`crates/android`, fourni)

`libtwitchminer_jni.so` est produite par la CI (artefact Android) ou par :
```bash
cargo ndk --target aarch64-linux-android --platform 26 build --release -p tm-android
```
Côté Kotlin, copier `crates/android/kotlin/Native.kt` :

```kotlin
val h = Native.start(filesDir.absolutePath, null)            // "0.0.0.0:7878" pour pilotage à distance
val reply = Native.execute(h, """{"cmd":"login_start"}""")    // {"ok":true,"data":{...}}
val summary = Native.summary(h)                               // pour la notification / Now Bar
Native.stop(h)
```

Toutes les fonctions échangent le JSON de [API.md](API.md). Le moteur tourne sur son propre runtime (2 threads) : `execute` est bloquant, à appeler hors du thread UI. Le service Kotlin fait `start` dans `onCreate` et `stop` dans `onDestroy`. Il rafraîchit sa notification avec `summary`. Une UI Rust native dans le même processus peut utiliser `tm_client::Local` directement.

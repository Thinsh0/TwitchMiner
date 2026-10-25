# Android (et OneUI)

Le moteur compile déjà pour Android (CI : `aarch64-linux-android`, `x86_64-linux-android`). Il reste à l'embarquer dans une app.

## Architecture recommandée

```
APK
├── libtwitchminer_jni.so  (Rust : tm-app + UI native, ex. Slint via android-activity)
│     └── tm_app::App::start(AppOptions { data_dir: Some(filesDir), api: None, .. })
└── MinerService.kt    (~100 lignes : foreground service + notification)
```

- **Un seul processus** : l'UI Rust et le moteur partagent `Engine`, sans passer par le réseau. L'API HTTP reste optionnelle, utile pour piloter le téléphone depuis le PC.
- `data_dir` = `context.filesDir` (récupéré via JNI ou passé par le service). Tout y est : base SQLite et journaux.
- Le daemon `twitchminer` (artefact CI Android) tourne aussi tel quel dans **Termux**, si on veut zéro APK.

## Rester vivant en arrière-plan

1. **Foreground service** obligatoire, sinon Android tue le minage en quelques minutes.
   - Type `specialUse` (`android:foregroundServiceType="specialUse"` + `<property android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE" android:value="Background media watch for Twitch rewards"/>`).
   - Ne pas utiliser `dataSync` : depuis Android 15, il est limité à 6 h par 24 h.
   - Permissions : `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_SPECIAL_USE`, `POST_NOTIFICATIONS`, `INTERNET`, `WAKE_LOCK`.
2. **Exemption d'optimisation batterie** : `ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`.
3. **OneUI** : en plus, guider l'utilisateur vers *Paramètres → Batterie → Limites d'utilisation en arrière-plan → Applications jamais en veille*. Sans ça, OneUI met l'app en veille malgré tout. Deep link possible : `Intent("com.samsung.android.sm.ACTION_BATTERY")`, avec repli sur les réglages de l'app.
4. Wake lock partiel uniquement pendant le minage. Le moteur est très peu actif : un cycle réseau toutes les ~10 s par chaîne regardée (quelques requêtes `HEAD` légères).

## Notification : la Now Bar de OneUI

Sur Android 16 (OneUI 8+), une notification en cours qui affiche une **progression** peut être promue en **Live Update**. OneUI l'affiche alors dans la **Now Bar** de l'écran verrouillé.

- `Notification.ProgressStyle` (API 36) + `setRequestPromotedOngoing(true)` + permission `POST_PROMOTED_NOTIFICATIONS`.
- Données : `Command::Summary` / `tm_core::view::summary()` → `accounts[].drop` fournit `game`, `drop`, `progress` (0..1), `current_minutes/required_minutes`, `eta`, `image_url`.
- Sans drop en cours : texte « Points : +N cette session » et chaînes regardées (`accounts[].watching`).
- Mettre à jour la notification sur les événements `drop_progress`, `watch_started/stopped` et `drop_claimed`, au plus une fois toutes les 30 s.
- Sur Android < 16 : notification classique avec `setProgress`.

Autres intégrations possibles, toutes alimentées par `summary` :
- **Tuile de réglages rapides** (`TileService`) : pause/reprise, via `account_set_enabled` ;
- **Widget** d'écran d'accueil.

## Pont JNI (`crates/android`, fourni)

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

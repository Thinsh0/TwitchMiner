# Context - Monorepo Restructuring & Android Port

## Decision: Monorepo Directory Structure
To avoid code duplication while keeping the codebase clean, we decided to restructure the project:
- `core/`: Common Rust code (API client, configuration, and miner engines).
- `frontend/`: Shared React + TS + Tailwind frontend, which will be responsive for both desktop and mobile/tablet.
- `desktop/`: Tauri desktop app backend.
- `mobile/`: Tauri mobile app backend (Android).

## Decision: Background Persistence on Android
We'll utilize `tauri-plugin-background-service` to run a Foreground Service on Android. This displays a notification and prevents the OS from suspending the Rust process and the socket connections needed for Twitch mining.

## Decision: Auto-start
We'll implement an Android `BroadcastReceiver` listening to `BOOT_COMPLETED` that triggers the Foreground Service automatically at startup, removing the need for the user to open the app manually.

## Decision: Idiot-Proof Setup UI
To prevent background termination by aggressive OS battery managers, we will implement:
- Dynamic permission checks (programmatic validation of notifications and battery optimization status).
- Redirect to battery setting details directly using Android intent.
- Links to manufacturer-specific troubleshooting pages (`dontkillmyapp.com`) with a step-by-step assistant.

## Decision: Channel-based JNI wrappers
To resolve the Rust closure type mismatches with Tauri's JNI `exec` API, we wrapped the JNI calls in an immediately-invoked closure returning `Result` and transferred the result to the main thread via a `std::sync::mpsc::channel`.

## Decision: Monorepo NPM Routing & Paths
- Created a root `package.json` with scripts targeting `--prefix frontend`.
- Placed target-specific `package.json` files in `mobile/src-tauri/` and `desktop/src-tauri/` containing a `"tauri": "tauri"` script to allow Gradle and local CLI calls to execute from the proper context directory while resolving binaries upward.
- Changed `tauri.conf.json` build/dev commands to use `../../frontend` instead of `../frontend` to match relative path structures from within backend subfolders.

## Decision: Startup Crash Resolution (State Management & Plugin Ordering)
To prevent the app from crashing on start when `tauri-plugin-background-service` runs before Tauri's `.setup()` block executes:
1. Registered `MinerState` and `StreakMinerState` directly on `tauri::Builder` (using `.manage(...)`).
2. Reordered all utility plugins (store, logs, etc.) to register before the background service.
3. Implemented defensive `app.try_state()` checks in the background service thread.


- Added @tauri-apps/plugin-opener and @tauri-apps/plugin-clipboard-manager to replace shell.open and document.execCommand for Android compatibility.
- Handled Android safe area with custom padding classes.

- Fixed MobileSetupWizard z-index clipping by moving it to App.tsx root.
- Replaced separate copy button with clickable verification code.
- Added clipboard-manager and opener capabilities to default.json to fix link opening.

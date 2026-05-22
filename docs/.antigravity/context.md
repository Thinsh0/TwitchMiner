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

## Decision: Dynamic Notifications and Foreground Service Label Updates
To prevent cluttering the user's notification drawer with alerts for simple state changes:
- Created a custom JNI/Rust bridge command `update_foreground_label` to update the persistent foreground service notification's label on Android.
- Introduced a `silent` attribute to `ActivityEvent` so that normal state transitions are only written to the in-app log and persistent notification, but are filtered out from triggering native system popups.
- Configured native alerts (popups) to only trigger for critical events: claiming a drop or starting a new active drop/campaign.

## Decision: Suppress Startup Drop Notifications
To prevent notification spam when launching the app or starting the miner, we suppress drop alerts if the miner was not previously running. We also filter out any drops that are already at 100% progress.

## Decision: Claiming Command and Frontend Binding
We expose a `claim_drop_cmd` Tauri command that calls Twitch's GQL `claim_drop` using the verified sha256 hash `2f884fa187b8fadb2a49db0adc033e636f7b6aaee6e76de1e2bba9a7baf0daf6`. We bind the manual "Claim" button in `DropsView.tsx` to this command, showing a loading indicator during the API call.

## Decision: Work Schedule System
We implement a local-time based work schedule system. The weekly intervals (Monday-Sunday) and exceptions (specific YYYY-MM-DD dates) are stored in the global configuration and checked in both Drop Miner and Streak Miner loop runs to suspend mining when outside allowed hours.

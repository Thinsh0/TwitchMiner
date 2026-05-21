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


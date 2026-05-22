# Plan - Monorepo Restructuring & Android Port

## Tasks
- [x] Move current frontend files into `frontend/`
- [x] Create `core/` shared Rust library and move miner logic there
- [x] Move Tauri desktop files to `desktop/` and link to `core` and `frontend`
- [x] Initialize Tauri Android project in `mobile/`
- [x] Implement Foreground Service and Auto-start Receiver on Android
- [x] Implement programmatic checks for Battery Optimizations & Notifications
- [x] Adapt frontend layout to be responsive for mobile/tablet
- [x] Add mobile configuration helper UI (idiot-proof guide with live green indicators)
- [x] Fix Android startup crash (state management race and plugin ordering)

## Status
The Android app now starts successfully without crashing. The fix for the `tauri-plugin-background-service` configuration and plugin ordering resolved the initialization issues.




- [x] Fix Android black screen crash via stable plugins & windows config.
- [x] Mobile QoL updates (Opener, Clipboard, Onboarding, Safe Area, Settings cleanup).

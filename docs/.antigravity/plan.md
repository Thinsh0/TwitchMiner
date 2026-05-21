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
- [x] Verify build and functionality on desktop and Android

## Status
Verification builds for both Desktop and Android packages are fully successful. Created delegate package.json files in the root and backend folders to handle monorepo path routing correctly during Gradle/Tauri cli execution. Verified that Android compiles to APK/AAB and Desktop compiles to MSI/setup EXE.




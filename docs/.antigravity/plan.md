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
- [ ] Verify build and functionality on desktop and Android

## Status
Safety commit created (`b7aceb6`). Mobile initialization, background service configuration, battery optimization JNI hooks, responsive layout, and interactive Setup Wizard with live indicators successfully implemented and committed (`a2368d0`).



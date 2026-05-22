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

- [x] Fix startup drop notifications
- [x] Shorten configuration status messages
- [x] Fix manual and automatic drop claiming
- [x] Implement work schedule system with exceptions

## Status
Completed the fixes for startup notifications, shortening status strings, resolving manual/automatic claiming, and implementing the work schedule system.

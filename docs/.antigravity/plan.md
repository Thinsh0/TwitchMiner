# Plan - Monorepo Restructuring & Android Port

## Tasks
- [x] Move current frontend files into `frontend/`
- [x] Create `core/` shared Rust library and move miner logic there
- [x] Move Tauri desktop files to `desktop/` and link to `core` and `frontend`
- [ ] Initialize Tauri Android project in `mobile/`
- [ ] Implement Foreground Service and Auto-start Receiver on Android
- [ ] Implement programmatic checks for Battery Optimizations & Notifications
- [ ] Adapt frontend layout to be responsive for mobile/tablet
- [ ] Add mobile configuration helper UI (idiot-proof guide with live green indicators)
- [ ] Verify build and functionality on desktop and Android

## Status
Safety commit created (`b7aceb6`). Monorepo plan updated to include programmatic checkmarks and brand guides for the mobile setup helper. Awaiting user approval.

# Plan - Port to Android (Background & Autostart)

## Tasks
- [ ] Initialize Android target (`npx tauri android init`)
- [ ] Implement Foreground Service in Kotlin for background persistence
- [ ] Implement Autostart BroadcastReceiver (`BOOT_COMPLETED`)
- [ ] Configure `AndroidManifest.xml` (Permissions & Services)
- [ ] Bridge Rust mining logic to Native Service
- [ ] Create Mobile Setup Helper UI (Battery, Notifications, Autostart)
- [ ] Verify background operation and autostart functionality
- [ ] Build and test APK

## Status
Starting Android porting process. Implementation plan created and awaiting user approval.

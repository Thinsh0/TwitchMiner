# Context - Port to Android

## Decision: Use Tauri 2.0 Mobile (Android)
We are using Tauri 2.0's native mobile support to port the application. This allows us to keep the existing Rust mining logic while providing a native Android experience.

## Decision: Foreground Service for Persistence
Android's background restrictions require a Foreground Service with a persistent notification to keep the process alive while the UI is closed. This ensures heartbeats continue even when the user isn't looking at the phone.

## Decision: Autostart via BroadcastReceiver
To fulfill the "automatic start" requirement, we'll implement a `RECEIVE_BOOT_COMPLETED` listener in Kotlin to launch the background service immediately after device startup.

## Decision: Resource Optimization
We will focus on keeping the background memory footprint low by ensuring the mining logic is efficient and doesn't trigger unnecessary WebView re-renders when in the background.

## Technical Trade-offs
- **Battery vs. Mining**: Background mining is battery-intensive due to constant network heartbeats. We'll guide the user to disable battery optimization.
- **Complexity**: Native Kotlin code is required to bridge Tauri with Android's system services (Service, Receiver).

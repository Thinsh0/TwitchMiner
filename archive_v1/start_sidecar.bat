@echo off
title Twitch Miner Sidecar Launcher
echo [Launcher] Starting Tauri App (Backend will be spawned automatically)...
cd tauri_ui
npm run tauri dev
pause

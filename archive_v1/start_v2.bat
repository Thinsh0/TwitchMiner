@echo off
title Twitch Miner v2 Launcher
echo [Launcher] Starting Backend...
:: Start Backend in a minimized separate window
start "Twitch Miner API" /min python main.py --server

echo [Launcher] Waiting for API...
timeout /t 3 /nobreak >nul

echo [Launcher] Starting Frontend (Dev Mode)...
cd tauri_ui
:: Start Tauri in current window (or use start to separate)
npm run tauri dev
pause

$ErrorActionPreference = "Stop"
Write-Host "Reconnecting ADB and waiting for device..."
& "C:\Users\enzom\AppData\Local\Android\Sdk\platform-tools\adb.exe" wait-for-device

Write-Host "Aligning APK..."
& "C:\Users\enzom\AppData\Local\Android\Sdk\build-tools\34.0.0\zipalign.exe" -p -f 4 "D:\Devs\TwitchMiner\mobile\src-tauri\gen\android\app\build\outputs\apk\universal\release\app-universal-release-unsigned.apk" "D:\Devs\TwitchMiner\mobile\src-tauri\gen\android\app\build\outputs\apk\universal\release\app-aligned.apk"

Write-Host "Signing APK with debug keystore..."
& "C:\Users\enzom\AppData\Local\Android\Sdk\build-tools\34.0.0\apksigner.bat" sign --ks "C:\Users\enzom\.android\debug.keystore" --ks-pass pass:android --key-pass pass:android "D:\Devs\TwitchMiner\mobile\src-tauri\gen\android\app\build\outputs\apk\universal\release\app-aligned.apk"

Write-Host "Uninstalling old APK..."
& "C:\Users\enzom\AppData\Local\Android\Sdk\platform-tools\adb.exe" uninstall com.twitchminer.mobile

Write-Host "Installing APK..."
& "C:\Users\enzom\AppData\Local\Android\Sdk\platform-tools\adb.exe" install -r -d "D:\Devs\TwitchMiner\mobile\src-tauri\gen\android\app\build\outputs\apk\universal\release\app-aligned.apk"

Write-Host "Starting App..."
& "C:\Users\enzom\AppData\Local\Android\Sdk\platform-tools\adb.exe" shell monkey -p com.twitchminer.mobile -c android.intent.category.LAUNCHER 1
Write-Host "Done!"
& "C:\Users\enzom\AppData\Local\Android\Sdk\platform-tools\adb.exe" shell monkey -p com.twitchminer.mobile -c android.intent.category.LAUNCHER 1
Write-Host "Done!"

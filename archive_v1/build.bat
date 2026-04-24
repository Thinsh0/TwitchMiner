@echo off
echo Building TwitchMiner...
python -m PyInstaller --noconfirm --onefile --windowed --name "Twitch Miner" --add-data "C:\Users\enzom\AppData\Local\Python\pythoncore-3.14-64\Lib\site-packages\customtkinter;customtkinter" main.py
echo Build Complete. Check 'dist' folder.
pause

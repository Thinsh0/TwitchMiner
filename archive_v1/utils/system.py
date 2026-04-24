import os
import sys
import threading
from PIL import Image, ImageDraw
import pystray
from pystray import MenuItem as item
import winshell
from win32com.client import Dispatch

class SystemManager:
    def __init__(self, app, config_manager, localization_manager):
        self.app = app
        self.config = config_manager
        self.loc = localization_manager
        self.icon = None
        self.tray_thread = None

    def create_image(self):
        # Generate a simple icon if none exists
        # In production, load from file
        width = 64
        height = 64
        color1 = "#9146FF" # Twitch Purple
        color2 = "white"
        
        image = Image.new('RGB', (width, height), color1)
        dc = ImageDraw.Draw(image)
        dc.rectangle(
            (width // 4, height // 4, width * 3 // 4, height * 3 // 4),
            fill=color2)
            
        return image

    def setup_tray(self):
        def on_quit(icon, item):
            icon.stop()
            self.app.quit() # Real quit

        def on_show(icon, item):
            self.app.show_window()

        image = self.create_image()
        menu = (
             item('Show', on_show, default=True),
             item('Quit', on_quit)
        )
        self.icon = pystray.Icon("name", image, "Twitch DropMiner", menu)
        
    def run_tray(self):
        self.tray_thread = threading.Thread(target=self.icon.run, daemon=True)
        self.tray_thread.start()
        
    def stop_tray(self):
        if self.icon:
            self.icon.stop()

    def set_startup(self, enable=True):
        startup_folder = winshell.startup()
        path = os.path.join(startup_folder, 'TwitchDropMiner.lnk')
        
        if enable:
            target = sys.executable
            # If running from script, target python and arg main.py
            # If frozen exe, target exe
            
            # Simple approach for dev environment:
            wDir = os.getcwd()
            target_script = os.path.join(wDir, "main.py")
            
            shell = Dispatch('WScript.Shell')
            shortcut = shell.CreateShortCut(path)
            shortcut.Targetpath = target
            shortcut.Arguments = f'"{target_script}"'
            shortcut.WorkingDirectory = wDir
            shortcut.IconLocation = target
            shortcut.save()
            print(f"[System] Startup shortcut created at {path}")
        else:
            if os.path.exists(path):
                os.remove(path)
                print(f"[System] Startup shortcut removed.")

    def check_startup(self):
        startup_folder = winshell.startup()
        path = os.path.join(startup_folder, 'TwitchDropMiner.lnk')
        return os.path.exists(path)

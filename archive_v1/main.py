import customtkinter as ctk
import sys
import threading
from ui.app import App
from utils.config import ConfigManager
from core.auth import DeviceAuthenticator
from core.miner import DropMiner
from core.bonus import BonusManager

if __name__ == "__main__":
    # Parse Args
    if len(sys.argv) > 1 and "--server" in sys.argv:
        print("[System] Starting in API Server Mode...")
        from core.api_server import start_api_server
        import time
        
        # Initialize Core without GUI
        config_manager = ConfigManager()
        authenticator = DeviceAuthenticator(config_manager)
        miner = DropMiner(config_manager)
        
        # Initialize Bonus Manager (Streaks/Farming)
        # Note: GQL might be None/Init pending if not authenticated, but Manager handles it.
        bonus_manager = BonusManager(config_manager, miner.gql, miner)
        
        # Start Server
        start_api_server(config_manager, authenticator, miner, bonus_manager)
        
        # Keep alive
        try:
            while True:
                time.sleep(1)
        except KeyboardInterrupt:
            print("Stopping...")
            miner.stop()
            
    else:
        # GUI Mode
        # Initialize Config
        config_manager = ConfigManager()
        
        # Initialize Auth
        authenticator = DeviceAuthenticator(config_manager)
        
        # Initialize Miner
        miner = DropMiner(config_manager)
        
        # Start GUI
        app = App(config_manager, authenticator, miner)
        app.mainloop()

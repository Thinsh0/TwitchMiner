import sys
from utils.config import ConfigManager
from core.auth import DeviceAuthenticator
from core.miner import DropMiner
from core.bonus import BonusManager
from core.api_server import start_api_server
import time
import threading

def main():
    print("[Sidecar] Starting Twitch Miner Backend...")
    
    # Initialize Core
    config_manager = ConfigManager()
    authenticator = DeviceAuthenticator(config_manager)
    miner = DropMiner(config_manager)
    
    # Initialize Bonus
    bonus_manager = BonusManager(config_manager, miner.gql, miner)
    
    # Start Server
    # We use a distinct port or the same one? Using 36969 as default.
    start_api_server(config_manager, authenticator, miner, bonus_manager)
    
    # Keep alive
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        print("[Sidecar] Stopping...")
        miner.stop()

if __name__ == "__main__":
    main()

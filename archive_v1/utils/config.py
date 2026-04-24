import json
import os
from pathlib import Path

CONFIG_FILE = "config.json"
# TwitchDropsMiner Android Client ID (Works better with integrity checks)
DEFAULT_CLIENT_ID = "kd1unb4b3q4t58fwlpcbzcbnm76a8fp" 

class ConfigManager:
    def __init__(self):
        self.config_path = Path(CONFIG_FILE)
        self.config = self.load_config()

    def load_config(self):
        if not self.config_path.exists():
            return self.create_default_config()
        try:
            with open(self.config_path, 'r') as f:
                config = json.load(f)
                # Force update Client ID if it doesn't match default (Fixes stale config issue)
                if config.get("client_id") != DEFAULT_CLIENT_ID:
                    print(f"DEBUG: Updating Client ID from {config.get('client_id')} to {DEFAULT_CLIENT_ID}. Clearing old auth.")
                    config["client_id"] = DEFAULT_CLIENT_ID
                    # Clear auth because token is tied to Client ID
                    config["auth"] = {
                        "access_token": None,
                        "refresh_token": None,
                        "expires_at": None
                    }
                    self.save_config(config)
                return config
        except json.JSONDecodeError:
            return self.create_default_config()

    def create_default_config(self):
        default_config = {
            "client_id": DEFAULT_CLIENT_ID,
            "auth": {
                "access_token": None,
                "refresh_token": None,
                "expires_at": None
            },
            "priorities": {
                "games": [],
                "streamers": []
            },
            "bonus": {
                "daily_streaks": {
                    "channels": [],
                    "history": {},
                    "auto_start": True,
                    "mode": "parallel" 
                },
                "points_farming": {
                     "channels": [],
                     "auto_start": False,
                     "max_concurrent": 3
                }
            },
            "settings": {
                "language": "EN",
                "check_interval": 60,
                "notifications_enabled": True,
                "notify_active_drop": True,
                "notify_claim": True,
                "notify_campaign_finish": True
            }
        }
        self.save_config(default_config)
        return default_config

    def save_config(self, config=None):
        if config:
            self.config = config
        with open(self.config_path, 'w') as f:
            json.dump(self.config, f, indent=4)

    def get(self, key, default=None):
        return self.config.get(key, default)

    def set(self, key, value):
        self.config[key] = value
        self.save_config()

    def get_auth_token(self):
        return self.config.get("auth", {}).get("access_token")

    def set_auth_token(self, access_token, refresh_token, expires_in):
        import time
        self.config["auth"] = {
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_at": time.time() + expires_in
        }
        self.save_config()

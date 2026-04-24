import requests
import time
import webbrowser
from utils.config import ConfigManager

class DeviceAuthenticator:
    def __init__(self, config_manager: ConfigManager):
        self.config = config_manager
        self.client_id = self.config.get("client_id")
        self.scopes = [
            "openid",
            "user:read:email"
            #"viewing_activity_read", 
            #"user:edit:broadcast",
            #"user:read:broadcast",
            #"chat:read",
            #"chat:edit",
            #"channel:read:subscriptions"
        ]
        
    def start_auth(self):
        """
        Initiates the Device Code Flow.
        Returns a dictionary with user_code, verification_uri, etc.
        """
        url = "https://id.twitch.tv/oauth2/device"
        payload = {
            "client_id": self.client_id,
            "scopes": " ".join(self.scopes)
        }
        # headers = {
        #     "User-Agent": "Mozilla/5.0 (Nintendo Switch; WifiWebAuthApp/3.0.0.5.9) AppleWebKit/537.36 (KHTML, like Gecko) NF/4.0.0.5.9 NintendoBrowser/5.1.0.13341"
        # }
        
        try:
            print(f"DEBUG: Starting Auth with ID: {self.client_id}")
            # Request WITHOUT custom headers primarily, as adding them caused 400 Invalid Client for this ID
            response = requests.post(url, data=payload)
            response.raise_for_status()
            data = response.json()
            print(f"DEBUG: Auth Init Success: {data}")
            return data
        except requests.exceptions.RequestException as e:
            print(f"Error starting auth: {e}")
            if 'response' in locals() and response is not None:
                print(f"Response Body: {response.text}")
            return None

    def poll_for_token(self, device_code, interval):
        """
        Polls Twitch API for the token.
        Blocks until token is received or expired.
        """
        url = "https://id.twitch.tv/oauth2/token"
        payload = {
            "client_id": self.client_id,
            "scopes": " ".join(self.scopes),
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
        }
        # headers = {
        #     "User-Agent": "Mozilla/5.0 (Nintendo Switch; WifiWebAuthApp/3.0.0.5.9) AppleWebKit/537.36 (KHTML, like Gecko) NF/4.0.0.5.9 NintendoBrowser/5.1.0.13341"
        # }
        
        print(f"DEBUG: Polling every {interval}s...")
        while True:
            time.sleep(interval)
            try:
                # Request WITHOUT custom headers to be safe
                response = requests.post(url, data=payload)
                if response.status_code == 200:
                    data = response.json()
                    print(f"DEBUG: Loop 200 OK! Token: {data.keys()}")
                    # Save tokens
                    self.config.set_auth_token(
                        data.get("access_token"), 
                        data.get("refresh_token"), 
                        data.get("expires_in", 3600) # Default to 1 hour if missing
                    )
                    return True, "Authenticated successfully!"
                
                elif response.status_code == 400:
                    error_data = response.json()
                    error = error_data.get("message", "")
                    print(f"DEBUG: Poll 400: {error_data}")
                    if error == "authorization_pending":
                        continue
                    elif error == "slow_down":
                        interval += 2
                        print(f"DEBUG: Slowing down (interval={interval})")
                        continue
                    elif error == "expired_token":
                        return False, "Code expired. Please try again."
                    else:
                        return False, f"Auth Error: {error}"
                else:
                    print(f"DEBUG: Unexpected Poll response {response.status_code}: {response.text}")
                    return False, f"Unexpected error: {response.status_code}"
            except Exception as e:
                return False, f"Connection error: {e}"

    def open_browser(self, uri):
        webbrowser.open(uri)

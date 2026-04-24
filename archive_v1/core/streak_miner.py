import threading
import time
from core.api import TwitchGQL

class StreakMiner:
    def __init__(self, config_manager, api_client: TwitchGQL):
        self.config = config_manager
        self.gql = api_client
        self.is_running = False
        self.thread = None
        self.status = "Idle"

    def start(self):
        if not self.is_running:
            self.is_running = True
            self.thread = threading.Thread(target=self._run_loop, daemon=True)
            self.thread.start()
            print("[StreakMiner] Started.")

    def stop(self):
        self.is_running = False
        self.status = "Stopped"
        print("[StreakMiner] Stopped.")

    def _run_loop(self):
        while self.is_running:
            channels = self.config.get("streaks", {}).get("channels", [])
            mode = self.config.get("streaks", {}).get("mode", "sequential")

            if not channels:
                self.status = "No channels configured."
                time.sleep(5)
                continue

            if mode == "parallel":
                print("[StreakMiner] Parallel mode activated (Simulation: Processing all at once)")
                # For now, sequential logic is safer for single account to avoid bans/issues, 
                # but we simulate parallel via log intent.
                # In real parallel, we'd spawn a thread per channel.
            
            for channel in channels:
                if not self.is_running: break
                
                self.status = f"Pinging {channel}..."
                print(f"[StreakMiner] Processing {channel}...")
                
                # 1. Get Spade URL
                spade_url = self.gql.get_spade_url(channel)
                
                # 2. Get User ID (Cache this later?)
                user_info = self.gql.get_user_info()
                user_id = user_info.get("data", {}).get("currentUser", {}).get("id") if user_info else None
                
                # 3. Get Stream Info for Broadcast ID
                stream_info = self.gql.get_stream_info(channel)
                # Need to parse this safely
                try:
                    user_data = stream_info.get("data", {}).get("user", {})
                    stream_data = user_data.get("stream")
                    
                    if stream_data:
                        broadcast_id = stream_data.get("id")
                        channel_id = user_data.get("id")
                        
                        if spade_url and user_id and broadcast_id and channel_id:
                            payload = {
                                "event": "minute-watched",
                                "properties": {
                                    "broadcast_id": str(broadcast_id),
                                    "channel_id": str(channel_id),
                                    "channel": channel,
                                    "hidden": False,
                                    "live": True,
                                    "location": "channel",
                                    "logged_in": True,
                                    "muted": False,
                                    "player": "site",
                                    "user_id": str(user_id),
                                }
                            }
                            # Send Ping
                            sent = self.gql.send_spade_minute_watched(spade_url, payload)
                            if sent:
                                print(f"[StreakMiner] Sent Ping for {channel}")
                            else:
                                print(f"[StreakMiner] Failed Ping for {channel}")
                        else:
                             print(f"[StreakMiner] Missing data for {channel} (Offline?)")
                    else:
                        print(f"[StreakMiner] {channel} is Offline.")
                        
                except Exception as e:
                    print(f"[StreakMiner] Error processing {channel}: {e}")
                
                # Wait a bit between channels to avoid rate limits
                time.sleep(2)

            self.status = "Waiting for next cycle..."
            # Wait 60s before next round of pings (Minute Watched)
            for _ in range(60):
                if not self.is_running: break
                time.sleep(1)

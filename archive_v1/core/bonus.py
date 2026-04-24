import threading
import time
import datetime
from core.api import TwitchGQL
from utils.notifications import NotificationManager

class DailyStreakService:
    def __init__(self, manager):
        self.manager = manager
        self.gql = manager.gql
        self.config = manager.config
        self.notifier = NotificationManager(self.config)
        
        self.is_running = False
        self.current_channel = None # Used for Sequential UI display
        self.status = "Idle"
        
        # Check Auto-Start
        if self.config.get("bonus", {}).get("daily_streaks", {}).get("auto_start", True):
             # Launch in separate thread to not block init
             threading.Thread(target=self.start_routine, daemon=True).start()

    def start_routine(self):
        if self.is_running: return
        self.run_daily_routine()

    def run_daily_routine(self):
        self.is_running = True
        self.status = "Starting Daily Routine..."
        
        streaks_cfg = self.config.get("bonus", {}).get("daily_streaks", {})
        channels = streaks_cfg.get("channels", [])
        history = streaks_cfg.get("history", {})
        mode = streaks_cfg.get("mode", "parallel") # sequential or parallel
        
        today_str = datetime.date.today().isoformat()
        
        to_process = [ch for ch in channels if history.get(ch) != today_str]
        
        if not to_process:
             self.status = "All daily streaks completed."
             self.is_running = False
             return

        if mode == "parallel":
            threads = []
            for ch in to_process:
                t = threading.Thread(target=self._process_single_streak, args=(ch,), daemon=True)
                t.start()
                threads.append(t)
            
            self.status = f"Processing {len(to_process)} streaks (Parallel)..."
            
            # Wait for all? Or just let them run. Let's wait to update global status.
            for t in threads: 
                if self.is_running: t.join()
        
        else: # Sequential
            for channel in to_process:
                if not self.is_running: break
                self.current_channel = channel
                self.status = f"Watching {channel} (Daily)..."
                self._process_single_streak(channel)

        if self.is_running:
             self.status = "Daily Routine Completed."
             self.is_running = False
             self.current_channel = None

    def _process_single_streak(self, channel):
        channel = channel.lower() 
        print(f"[DailyStreak] Starting watch for {channel}...")
        
        # Get current local streak
        current_val = self._get_local_streak(channel)
        
        # Watch Logic (6 minutes)
        # We check self.is_running inside _watch_channel loop
        success = self.manager._watch_channel(channel, duration_minutes=6)
        
        if success and self.is_running:
            print(f"[DailyStreak] {channel} Done!")
            
            bonus = self.config.get("bonus", {})
            hist = bonus.get("daily_streaks", {}).get("history", {})
            
            hist[channel] = datetime.date.today().isoformat()
            
            # Update 'streak_values' in config (Local Counter)
            if "values" not in bonus["daily_streaks"]: bonus["daily_streaks"]["values"] = {}
            new_val = current_val + 1
            bonus["daily_streaks"]["values"][channel] = new_val
            
            self.config.set("bonus", bonus)
            
            # NOTIFICATION
            self.notifier.send_notification("Streak Completed!", f"Daily watch for {channel} finished. (Streak: {new_val})")

        else:
             print(f"[DailyStreak] Failed/Stopped/Offline for {channel}.")

    def _get_local_streak(self, channel):
        # Return local counter
        vals = self.config.get("bonus", {}).get("daily_streaks", {}).get("values", {})
        return vals.get(channel, 0)
    
    def _try_get_streak_value(self, channel):
         # DEPRECATED/UNUSED for now
         return 0 


class PointsFarmingService:
    def __init__(self, manager):
        self.manager = manager
        self.gql = manager.gql
        self.config = manager.config
        
        self.is_running = False
        self.threads = []
        self.stats = {} 
        
        # Auto Start
        if self.config.get("bonus", {}).get("points_farming", {}).get("auto_start", False):
             threading.Thread(target=self.start_farming, daemon=True).start()

    def start_farming(self):
        if self.is_running: return
        self.is_running = True
        
        points_cfg = self.config.get("bonus", {}).get("points_farming", {})
        channels = points_cfg.get("channels", [])
        max_concurrent = points_cfg.get("max_concurrent", 3)
        
        self.semaphore = threading.Semaphore(max_concurrent)
        self.threads = []
        
        for channel in channels:
            t = threading.Thread(target=self._farm_channel_managed, args=(channel,), daemon=True)
            t.start()
            self.threads.append(t)
            
    def stop_farming(self):
         self.is_running = False
         # Threads will exit loop
         
    def _farm_channel_managed(self, channel):
        channel = channel.lower()
        # We loop here to handle switching between Active (Semaphore) and Passive (Piggyback) modes
        while self.is_running:
            is_piggy = False
            # Check if DropMiner is already watching this channel
            if self.manager.miner and self.manager.miner.current_streamer_login:
                if self.manager.miner.current_streamer_login.lower() == channel:
                    is_piggy = True

            if is_piggy:
                # PASSIVE MODE: No Semaphore needed
                self._run_farm_loop(channel, mode="passive")
            else:
                # ACTIVE MODE: Needs Semaphore
                self.stats[channel] = {"points": "Queued", "status": "Waiting..."}
                
                # Try to acquire semaphore with timeout to allow re-checking drop status
                acquired = self.semaphore.acquire(timeout=2.0)
                if acquired:
                    try:
                        if not self.is_running: break
                        self._run_farm_loop(channel, mode="active")
                    finally:
                        self.semaphore.release()
                else:
                    # Timeout, just loop back to check is_running & drop status
                    continue

            time.sleep(1)

    def _run_farm_loop(self, channel, mode="active"):
        """
        Runs the farming loop for a specific mode. 
        Returns if the mode is no longer valid (e.g. drop miner started/stopped).
        """
        self.stats[channel] = {"points": "Loading...", "status": f"Starting ({mode})..."}
        
        while self.is_running:
            # 1. Check consistency to switch modes
            is_piggy = False
            if self.manager.miner and self.manager.miner.current_streamer_login:
                if self.manager.miner.current_streamer_login.lower() == channel:
                    is_piggy = True
            
            # If active but should be passive -> Return to switch
            if mode == "active" and is_piggy: return
            # If passive but drops stopped -> Return to switch (acquire semaphore)
            if mode == "passive" and not is_piggy: return

            # 2. Update Points
            try:
                data = self.gql.get_channel_points_context(channel)
                if data:
                    ch_data = data.get("data", {}).get("channel", {})
                    if ch_data and ch_data.get("self"):
                        cp = ch_data["self"].get("communityPoints", {})
                        pts = cp.get("balance", 0)
                        
                        # Formatting
                        if isinstance(pts, int) and pts >= 1000:
                            self.stats[channel]["points"] = f"{pts/1000:.1f}K"
                        else:
                            self.stats[channel]["points"] = str(pts)
                            
                        # Auto-Claim Bonus (Green Chest)
                        claim = cp.get("availableClaim")
                        if claim:
                            claim_id = claim.get("id")
                            chan_id = ch_data.get("id")
                            if claim_id and chan_id:
                                # print(f"[Bonus] Claiming chest for {channel}...")
                                self.gql.claim_community_points(chan_id, claim_id)
                                self.stats[channel]["status"] = "Chest Claimed!"
                    else:
                        self.stats[channel]["points"] = "Limit/Err"
                else:
                    self.stats[channel]["points"] = "Err"
            except Exception as e:
                self.stats[channel]["points"] = "?"
            
            # 3. Watch / Sleep
            if mode == "passive":
                self.stats[channel]["status"] = "Mining (via Drops)"
                time.sleep(30) # Updates slowly, no resources used
            else:
                self.stats[channel]["status"] = "Watching..."
                self.manager._watch_channel(channel, duration_minutes=1, single_ping=True)
                time.sleep(5)
            
        self.stats[channel]["status"] = "Stopped"


class BonusManager:
    def __init__(self, config_manager, api_client: TwitchGQL, miner=None):
        self.config = config_manager
        self.gql = api_client
        self.miner = miner # Reference to DropMiner
        
        self.daily = DailyStreakService(self)
        self.points = PointsFarmingService(self)

    def _watch_channel(self, channel, duration_minutes=5, single_ping=False):
        """Shared logic."""
        try:
            spade_url = self.gql.get_spade_url(channel)
            # Optimization: Cache user_id?
            # For now fetch fresh
            user_info = self.gql.get_user_info()
            user_id = user_info.get("data", {}).get("currentUser", {}).get("id") if user_info else None
            
            stream_info = self.gql.get_stream_info(channel)
            user_data = stream_info.get("data", {}).get("user", {})
            stream_data = user_data.get("stream")
            
            if not (spade_url and user_id and user_data):
                return False

            broadcast_id = stream_data.get("id") if stream_data else "0" 
            
            # CRITICAL: Strict Live Check
            # If broadcast_id is "0", stream is offline.
            if broadcast_id == "0":
                 # print(f"[BonusManager] Channel {channel} is Offline. Skipping.")
                 return False

            channel_id = user_data.get("id")
            
            payload = {
                "event": "minute-watched",
                "properties": {
                    "broadcast_id": str(broadcast_id),
                    "channel_id": str(channel_id),
                    "channel": channel,
                    "hidden": False,
                    "live": (broadcast_id != "0"),
                    "location": "channel",
                    "logged_in": True,
                    "muted": False,
                    "player": "site",
                    "user_id": str(user_id),
                }
            }

            loops = 1 if single_ping else duration_minutes
            
            for i in range(loops):
                if single_ping and not self.points.is_running: break
                if not single_ping and not self.daily.is_running: break

                self.gql.send_spade_minute_watched(spade_url, payload)
                
                if i < loops - 1 or not single_ping:
                     for _ in range(60):
                         if single_ping and not self.points.is_running: break
                         if not single_ping and not self.daily.is_running: break
                         time.sleep(1)
            
            return True
            
        except Exception as e:
            print(f"[BonusManager] Watch error for {channel}: {e}")
            return False

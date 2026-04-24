import time
import threading
from core.api import TwitchGQL
from utils.config import ConfigManager

from utils.notifications import NotificationManager

class DropMiner:
    def __init__(self, config: ConfigManager):
        self.config = config
        self.gql = TwitchGQL(config)
        self.notifier = NotificationManager(config)
        self.is_running = False
        self.thread = None
        self.status = "Idle"
        self.current_streamer = None
        self.current_streamer_login = None
        self.streamer_image_url = None
        self.current_drop = None
        self.drop_image_url = None
        self.progress = 0
        self.campaign_progress = 0
        self.current_points = 0

    def start(self):
        if not self.is_running:
            self.is_running = True
            self.thread = threading.Thread(target=self._run_loop, daemon=True)
            self.thread.start()

    def stop(self):
        self.is_running = False

    def get_status(self):
        return {
            "status": self.status,
            "streamer": self.current_streamer,
            "streamer_image_url": self.streamer_image_url,
            "drop": self.current_drop,
            "progress": self.progress,
            "campaign_progress": self.campaign_progress,
            "image_url": self.drop_image_url,
            "current_points": self.current_points
        }

    def _run_loop(self):
        while self.is_running:
            try:
                # Configuration Check
                mode = self.config.get("settings", {}).get("priority_mode", "Drops Only")
                
                # Check Inventory & Claim
                self.status = "Checking Inventory..."
                ui_res = self.gql.get_user_info() # Keep session alive & get ID
                current_user_id = ui_res.get("data", {}).get("currentUser", {}).get("id") if ui_res else None
                
                inventory = None
                active_campaigns = None
                
                # SMART MODE / DROPS ONLY: Always fetch drops
                inventory = self.gql.get_inventory()
                active_campaigns = self.gql.get_drops_campaigns()
                
                # Check for claimable drops
                if inventory and current_user_id:
                    # 1. Check "drops" list (Standard Completed List)
                    drops_list = inventory.get("data", {}).get("currentUser", {}).get("inventory", {}).get("drops", [])
                    for drop in drops_list:
                        self_data = drop.get("self", {})
                        is_claimed = self_data.get("isClaimed", False)
                        pct = drop.get("percentAwarded", 0)
                        
                        if not is_claimed and pct >= 100:
                            print(f"[Miner] Found claimable drop in list: {drop['name']}")
                            claim_id = self_data.get("dropInstanceID")
                            
                            # GENERATE CLAIM ID IF MISSING (TwitchDropsMiner logic)
                            if not claim_id:
                                camp_id = drop.get("campaign", {}).get("id")
                                drop_def_id = drop.get("id")
                                if camp_id and drop_def_id:
                                    claim_id = f"{current_user_id}#{camp_id}#{drop_def_id}"
                                    print(f"[Miner] Generated missing claim_id: {claim_id}")

                            if claim_id:
                                res = self.gql.claim_drop(claim_id)
                                
                                # Check if failed
                                if res and "errors" in res:
                                    print(f"[Miner] Standard claim failed: {res['errors']}")
                                else:
                                    # SUCCESS NOTIFICATION
                                    self.notifier.send_notification(
                                        "Drop Claimed!", 
                                        f"Successfully claimed: {drop['name']}", 
                                        event_type="claim"
                                    )
                                    
                                time.sleep(2)
                    
                    # 2. Check "dropCampaignsInProgress" (Active List - often has stuck claims)
                    active_campaigns_inv = inventory.get("data", {}).get("currentUser", {}).get("inventory", {}).get("dropCampaignsInProgress", [])
                    if active_campaigns_inv:
                        for camp in active_campaigns_inv:
                            time_drops = camp.get("timeBasedDrops", [])
                            for d in time_drops:
                                self_data = d.get("self", {})
                                is_claimed = self_data.get("isClaimed", False)
                                cur_mins = self_data.get("currentMinutesWatched", 0)
                                req_mins = d.get("requiredMinutesWatched", 0)
                                if not is_claimed and req_mins > 0 and cur_mins >= req_mins:
                                     drop_name_safe = d['name'].encode('ascii', 'ignore').decode('ascii')
                                     print(f"[Miner] Found claimable drop in Campaign Progress: {drop_name_safe}")
                                     
                                     claim_id = self_data.get("dropInstanceID")
                                     # GENERATE CLAIM ID IF MISSING
                                     if not claim_id:
                                        camp_id = camp.get("id")
                                        drop_def_id = d.get("id")
                                        if camp_id and drop_def_id:
                                            claim_id = f"{current_user_id}#{camp_id}#{drop_def_id}"
                                            print(f"[Miner] Generated missing claim_id: {claim_id}")


                                         
                                     if claim_id:
                                         res = self.gql.claim_drop(claim_id)
                                         print(f"[Miner] Campaign Progress Claim Result: {res}")
                                         
                                         if res and "errors" in res:
                                             print(f"[Miner] Claim failed: {res['errors']}")
                                         else:
                                             # SUCCESS NOTIFICATION
                                             drop_name_clean = d['name'].encode('ascii', 'ignore').decode('ascii')
                                             self.notifier.send_notification(
                                                "Drop Claimed!", 
                                                f"Successfully claimed: {drop_name_clean}", 
                                                event_type="claim"
                                             )
                                         
                                         time.sleep(2)
                                     else:
                                         print(f"[Miner Warning] No claim_id generated for {drop_name_safe}. Trying legacy claim with definition ID (Unlikely to work)...")
                                         # self.gql.claim_drop_legacy(d["id"]) # Probably useless if we need instance ID
                                         pass


                

                # 2. Find a Game to Mine
                target_game = None
                target_drop_name = "Unknown Drop"
                
                # A) Check Priority List Games
                priority_games = self.gql.config.get("priorities", {}).get("games", [])
                
                campaigns_list = active_campaigns.get("data", {}).get("currentUser", {}).get("dropCampaigns", []) if active_campaigns else []

                for p_game in priority_games:
                    # Find game in active campaigns to get details
                    found_c = None
                    if campaigns_list:
                        for c in campaigns_list:
                             if c["game"]["displayName"].lower() == p_game.lower():
                                 found_c = c
                                 # NOTIFY FOUND CAMPAIGN (First time logic would be complex, just notify on connect/start is easier? 
                                 # Or better: Notify when we START mining it)
                                 break
                    
                    if found_c:
                        # Check drops logic...
                        pass
                    
                    game_id = None
                    game_slug = None
                    
                    if found_c:
                         game_id = found_c["game"]["id"]
                         # Try to find slug, or generate it
                         game_slug = found_c["game"].get("name") or self.gql.get_slug(found_c["game"]["displayName"])
                    else:
                        # Search for it
                        print(f"[Miner] Priority game '{p_game}' not in active campaigns. Searching...")
                        search_res = self.gql.search_games(p_game)
                        try:
                            edges = search_res.get("data", {}).get("searchFor", {}).get("games", {}).get("edges", [])
                            if edges: 
                                item = edges[0]["item"]
                                game_id = item["id"]
                                # name is normally slug, but sometimes we might need to force it
                                game_slug = item.get("name")
                                if not game_slug:
                                     game_slug = self.gql.get_slug(item["displayName"])
                        except:
                            # Fallback if search structure weird
                            game_slug = self.gql.get_slug(p_game)
                            pass
                            
                        # Double check if search totally failed but we want to try blindly with generated slug
                        if not game_id and not game_slug:
                             game_slug = self.gql.get_slug(p_game)
                    
                    if game_id and game_slug:
                        target_game = {"id": game_id, "displayName": p_game, "slug": game_slug}
                        target_drop_name = "Priority: " + p_game
                        self.progress = 0
                        print(f"[Miner] Proceeding with Priority Game: {p_game} (ID: {game_id})")
                        break
                
                # B) If no priority found, check IN_PROGRESS drops (continue what we started)
                if not target_game and inventory:
                    drops = inventory.get("data", {}).get("currentUser", {}).get("inventory", {}).get("drops", [])
                    for drop in drops:
                        if drop["status"] == "IN_PROGRESS" and drop["percentAwarded"] < 100:
                            g = drop["campaign"]["game"]
                            target_game = {
                                "id": g["id"], 
                                "displayName": g["displayName"],
                                "slug": g.get("name") or self.gql.get_slug(g["displayName"])
                            }
                            target_drop_name = drop["name"]
                            self.progress = drop["percentAwarded"]
                            break
                            
                # C) If nothing, pick ANY active campaign (ONLY IF ENABLED)
                mine_unplanned = self.config.get("settings", {}).get("mine_unplanned_campaigns", False)
                
                if not target_game and active_campaigns and mine_unplanned:
                    campaigns = active_campaigns.get("data", {}).get("currentUser", {}).get("dropCampaigns", [])
                    
                    # DEBUG: List all active campaigns found
                    active_game_names = [c["game"]["displayName"] for c in campaigns if c["status"] == "ACTIVE"]

                    for camp in campaigns:
                        if camp["status"] == "ACTIVE":
                            g = camp["game"]
                            target_game = {
                                "id": g["id"], 
                                "displayName": g["displayName"],
                                "slug": g.get("name") or self.gql.get_slug(g["displayName"])
                            }
                            target_drop_name = "New Drop"
                            self.progress = 0
                            break
                elif not target_game and not mine_unplanned:
                     # Explicit logging for Idle state
                     pass

                # 3. Mine
                if target_game:
                    self.current_drop = target_drop_name
                    # Find Streamer
                    self.status = f"Looking for {target_game['displayName']} streamer..."
                    # REVERT: Use simple ID-based query for reliability
                    streams = self.gql.get_live_streams(target_game["id"])
                    
                    streamer_login = None
                    if streams:
                        edges = streams.get("data", {}).get("game", {}).get("streams", {}).get("edges", [])
                        
                        # Check Streamer Priority
                        priority_streamers = self.gql.config.get("priorities", {}).get("streamers", [])
                        for p_streamer in priority_streamers:
                            for edge in edges:
                                node = edge["node"]
                                if node["broadcaster"]["login"].lower() == p_streamer.lower():
                                    streamer_login = node["broadcaster"]["login"]
                                    streamer_id = node["broadcaster"]["id"] # Channel ID
                                    streamer_broadcast_id = node["id"] # Stream Broadcast ID
                                    self.current_streamer = node["broadcaster"]["displayName"] + " (Priority)"
                                    self.current_streamer_login = node["broadcaster"]["login"]
                                    self.streamer_image_url = node["broadcaster"].get("profileImageURL")
                                    break
                            if streamer_login: break
                        
                        # If no priority streamer, pick one with 'Drops Enabled' or top viewer
                        if not streamer_login:
                            for edge in edges:
                                node = edge["node"]
                                tags = node.get("tags", [])
                                has_drops = any(t["id"] == "c2542d6d-cd10-4532-919b-3d19f30a768b" for t in tags) # "Drops Enabled" tag UUID
                                # Pick if drops enabled or just fallback to first high viewer one
                                streamer_login = node["broadcaster"]["login"]
                                streamer_id = node["broadcaster"]["id"] # Channel ID
                                streamer_broadcast_id = node["id"] # Stream Broadcast ID
                                self.current_streamer = node["broadcaster"]["displayName"]
                                self.current_streamer_login = node["broadcaster"]["login"]
                                self.streamer_image_url = node["broadcaster"].get("profileImageURL")
                                break
                    
                    if streamer_login:
                        self.status = f"Watching {self.current_streamer}..."
                        print(f"[Miner] Fetching Playback Token for {streamer_login} (Registers View)")
                        self.gql.get_playback_access_token(streamer_login)
                        
                        # Get Spade URL and User ID for Telemetry
                        print(f"[Miner] Initializing Telemetry for {streamer_login}...")
                        spade_url = self.gql.get_spade_url(streamer_login)
                        user_info = self.gql.get_user_info()
                        user_id = user_info.get("data", {}).get("currentUser", {}).get("id") if user_info else None
                        
                        if not spade_url:
                            print("[Miner Warning] Could not find Spade URL. Progress might not track!")
                        if not user_id:
                            print("[Miner Warning] Could not find User ID. Progress might not track!")

                        # Initial fetch of progress
                        current_minutes = 0
                        required_minutes = 0
                        found_drop = False

                        def update_progress():
                            nonlocal current_minutes, required_minutes, found_drop
                            inv = self.gql.get_inventory()
                            if inv:
                                # New stricture from Persisted Query
                                campaigns = inv.get("data", {}).get("currentUser", {}).get("inventory", {}).get("dropCampaignsInProgress", [])
                                
                                if not campaigns:
                                    # Fallback or empty
                                    # print("[Miner DEBUG] No active campaigns in inventory response.")
                                    return False

                                # Determine Target Game ID as string
                                target_gid = str(target_game["id"]) if target_game else "0"

                                for camp in campaigns:
                                    game_id = str(camp.get("game", {}).get("id"))
                                    
                                    drops = camp.get("timeBasedDrops", [])
                                    
                                    is_target_game = (game_id == target_gid) 
                                    
                                    # Fallback: Check if ANY drop in this campaign matches self.current_drop if target_game is None/Zero
                                    if target_gid == "0" and self.current_drop:
                                         if any(d.get("name") == self.current_drop for d in drops):
                                             is_target_game = True
                                             # Fallback matched

                                    # Campaign Progress Calculation (Time Based)
                                    # Campaign Progress Calculation (Time Based)
                                    if is_target_game:
                                        if drops:
                                            total_mins = sum(d.get("requiredMinutesWatched", 0) for d in drops)
                                            # ... calculation ... (keep existing)
                                            current_mins_total = 0
                                            for d in drops:
                                                s = d.get("self", {})
                                                if s.get("isClaimed", False):
                                                    current_mins_total += d.get("requiredMinutesWatched", 0)
                                                else:
                                                    current_mins_total += s.get("currentMinutesWatched", 0)      
                                            


                                            if total_mins > 0:
                                                self.campaign_progress = (current_mins_total / total_mins) * 100
                                            else:
                                                self.campaign_progress = 0
                                        else:
                                            self.campaign_progress = 0

                                    for d in drops:
                                        # ... logic ...
                                        self_data = d.get("self", {})
                                        cur_mins = self_data.get("currentMinutesWatched", 0)
                                        req_mins = d.get("requiredMinutesWatched", 0)
                                        is_claimed = self_data.get("isClaimed", False)
                                        
                                        percent = 0
                                        if req_mins > 0:
                                            percent = (cur_mins / req_mins) * 100
                                        
                                        status = "IN_PROGRESS"
                                        if is_claimed: status = "CLAIMED"
                                        elif cur_mins >= req_mins: status = "CLAIMABLE"
                                        
                                        # If we don't have a target game yet (startup), but this is active/progressing, maybe we track it?
                                        # For now, strict 'is_target_game' logic applies if we are in 'watch' loop.
                                        
                                        if is_target_game:
                                            if status == "CLAIMED" or status == "CLAIMABLE":
                                                continue 
                                            
                                            self.current_drop = d["name"]
                                            
                                            # Extract Image with Fallback (Try benefitEdges -> benefit -> imageAssetURL)
                                            self.drop_image_url = None
                                            try:
                                                # Priority 1: Benefit Edges (Standard)
                                                edges = d.get("benefitEdges", [])
                                                if edges: 
                                                    # DEBUG: Print the first edge structure
                                                    # print(f"[Miner DEBUG] Edge[0]: {edges[0]}")
                                                    self.drop_image_url = edges[0].get("benefit", {}).get("imageAssetURL")
                                                
                                                # Priority 2: Direct Image
                                                if not self.drop_image_url:
                                                    self.drop_image_url = d.get("imageURL")
                                                    
                                            except Exception as e:
                                                print(f"[Miner DEBUG] Image Extraction Error: {e}")
                                            
                                            if self.drop_image_url:
                                                pass
                                                # print(f"[Miner] Image URL found: {self.drop_image_url}")
                                            else:
                                                pass
                                                # print(f"[Miner] Image URL NOT found.")

                                            current_minutes = cur_mins
                                            required_minutes = req_mins
                                            self.progress = percent
                                            
                                            found_drop = True
                                            return True
                                            
                                return False
                            return False

                        # Try to get existing progress
                        stats_found = update_progress()
                        
                        if not stats_found:
                            print(f"[Miner] Drop not in inventory yet. Starting Blind Watch to trigger it...")
                            self.current_drop = "Pending..."
                            current_minutes = 0
                            required_minutes = 120 # Default guess
                            self.progress = 0

                        # Proceed with mining even if blind
                        remaining_minutes = required_minutes - current_minutes
                        if remaining_minutes < 0: remaining_minutes = 0
                        
                        if stats_found:
                             self.status = f"Mining {self.current_drop}: {remaining_minutes}m left ({int(self.progress)}%)"
                        else:
                             self.status = f"Mining (Waiting for progress)..."
                             
                        try:
                            print(f"[Miner] {self.status.encode('ascii', 'ignore').decode('ascii')}")
                        except:
                            print("[Miner] Mining...")

                        # Watch Loop - Sync every minute
                        loop_limit = remaining_minutes if stats_found else 15
                        
                        while loop_limit > 0 and self.is_running:
                            # 1. Send Spade Telemetry (Minute Watched)
                            if spade_url and user_id and streamer_broadcast_id and streamer_id:
                                payload = {
                                    "event": "minute-watched",
                                    "properties": {
                                        "broadcast_id": str(streamer_broadcast_id),
                                        "channel_id": str(streamer_id),
                                        "channel": streamer_login,
                                        "hidden": False,
                                        "live": True,
                                        "location": "channel",
                                        "logged_in": True,
                                        "muted": False,
                                        "player": "site",
                                        "user_id": str(user_id),
                                    }
                                }
                                sent = self.gql.send_spade_minute_watched(spade_url, payload)
                                if sent:
                                    print(f"[Miner] Sent 'minute-watched' heartbeat to Spade.")
                                else:
                                    print(f"[Miner] Failed to send Spade heartbeat!")
                            else:
                                # Fallback if Spade missing (legacy/simple GQL ping)
                                self.gql.get_stream_info(streamer_login)
                                
                            # 2. Check/Claim Community Points (Green Chest) for Drop Channel
                            try:
                                pts_data = self.gql.get_channel_points_context(streamer_login)
                                if pts_data:
                                    ch_data = pts_data.get("data", {}).get("channel", {})
                                    if ch_data and ch_data.get("self"):
                                        cp = ch_data["self"].get("communityPoints", {})
                                        pts_bal = cp.get("balance", 0)
                                        
                                        # Format
                                        if isinstance(pts_bal, int) and pts_bal >= 1000:
                                            self.current_points = f"{pts_bal/1000:.1f}K"
                                        else:
                                            self.current_points = str(pts_bal)
                                        
                                        # Claim
                                        claim = cp.get("availableClaim")
                                        if claim:
                                            claim_id = claim.get("id")
                                            chan_id = ch_data.get("id")
                                            if claim_id and chan_id:
                                                print(f"[Miner] Claiming bonus chest for {streamer_login}!")
                                                self.gql.claim_community_points(chan_id, claim_id)
                            except Exception as e:
                                pass # Silent fail, non-critical
                            
                            # Sleep 60s
                            for _ in range(60):
                                if not self.is_running: break
                                time.sleep(1)
                            
                            loop_limit -= 1
                            
                            # Re-sync with Server to confirm progress
                            old_minutes = current_minutes
                            if update_progress():
                                # We found the drop! Update logic to become authoritative
                                stats_found = True
                                remaining_minutes = required_minutes - current_minutes
                                if remaining_minutes < 0: remaining_minutes = 0
                                loop_limit = remaining_minutes 
                                
                                # Update Status
                                self.status = f"Mining {self.current_drop}: {remaining_minutes}m left ({int(self.progress)}%)"
                                print(f"[Miner] Progress: {current_minutes}/{required_minutes}m ({int(self.progress)}%)")
                                
                                # INSTANT CLAIM TRIGGER
                                # If we reached target, break loop to go to 'Check Inventory & Claim' immediately
                                if current_minutes >= required_minutes:
                                    print("[Miner] Drop completed! Triggering claim cycle.")
                                    self.status = "Drop Completed. Claiming..."
                                    break
                                
                                # Check if stalled (only if we have history)
                                if current_minutes > 0 and current_minutes == old_minutes:
                                    print(f"[Miner Warning] Progress stalled at {current_minutes}m. Refreshing Streamer...")
                                    # Refreshing Spade URL might be needed if stream changed
                                    spade_url = self.gql.get_spade_url(streamer_login)
                            else:
                                if stats_found:
                                    print("[Miner] Drop info lost (completed?). Checking next...")
                                    break
                                else:
                                    print("[Miner] Still waiting for drop to appear in inventory...")

                    else:
                        self.status = f"No streamers found for {target_game['displayName']}"
                        self.current_streamer = None
                        time.sleep(60)

                else:
                    self.status = "No active drops found."
                    self.current_drop = None
                    self.current_streamer = None
                    time.sleep(60)
                    
            except Exception as e:
                import traceback
                traceback.print_exc()
                print(f"[Miner Error] {e}")
                self.status = f"Error: {str(e)[:20]}..."
                time.sleep(30)



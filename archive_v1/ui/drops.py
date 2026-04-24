import customtkinter as ctk
import threading
import requests
from PIL import Image
from io import BytesIO

class DropCard(ctk.CTkFrame):
    def __init__(self, master, drop_data, **kwargs):
        super().__init__(master, **kwargs)
        
        # Data extraction
        self.name = drop_data.get("name", "Unknown Drop")
        # Image Parsing Logic
        self.benefit_image_url = None
        
        # Check benefitEdges (Standard GQL structure)
        benefit_edges = drop_data.get("benefitEdges", [])
        if benefit_edges and len(benefit_edges) > 0:
            benefit = benefit_edges[0].get("benefit", {})
            self.benefit_image_url = benefit.get("imageAssetURL")
            
        # Fallback: direct benefit or imageURL
        if not self.benefit_image_url:
             self.benefit_image_url = drop_data.get("benefit", {}).get("imageAssetURL") or drop_data.get("imageURL")
        
        self_data = drop_data.get("self", {})
        self.current_minutes = self_data.get("currentMinutesWatched", 0)
        self.required_minutes = drop_data.get("requiredMinutesWatched", 0)
        self.is_claimed = self_data.get("isClaimed", False)
        
        # Calculate percentage
        self.percentage = 0
        if self.required_minutes > 0:
            self.percentage = min(1.0, self.current_minutes / self.required_minutes)
        
        # --- UI Layout ---
        self.grid_columnconfigure(1, weight=1) # Title/Bar expands
        
        # 1. Image Placeholder (Left)
        self.image_label = ctk.CTkLabel(self, text="...", width=60, height=60, fg_color="gray30")
        self.image_label.grid(row=0, column=0, rowspan=2, padx=5, pady=5)
        
        # 2. Title (Top Right)
        self.title_label = ctk.CTkLabel(self, text=self.name, font=("Roboto", 14, "bold"), anchor="w")
        self.title_label.grid(row=0, column=1, sticky="w", padx=5, pady=(5,0))
        
        # 3. Status/Progress (Bottom Right)
        status_text = f"{self.current_minutes}/{self.required_minutes} min"
        if self.is_claimed:
            status_text = "CLAIMED"
        elif self.percentage >= 1.0:
            status_text = "Ready to Claim"
            
        self.progress_bar = ctk.CTkProgressBar(self, width=200, height=10)
        self.progress_bar.set(self.percentage)
        self.progress_bar.grid(row=1, column=1, sticky="ew", padx=5, pady=(0,5))
        
        self.status_label = ctk.CTkLabel(self, text=status_text, font=("Roboto", 12))
        self.status_label.grid(row=1, column=2, padx=10)

        # Start image load
        if self.benefit_image_url:
            threading.Thread(target=self._load_image, daemon=True).start()

    def _load_image(self):
        try:
            response = requests.get(self.benefit_image_url, timeout=5)
            if response.status_code == 200:
                img_data = BytesIO(response.content)
                img = Image.open(img_data)
                # Resize
                img = img.resize((60, 60), Image.Resampling.LANCZOS)
                ctk_img = ctk.CTkImage(light_image=img, dark_image=img, size=(60, 60))
                
                # Update UI on main thread
                # Note: In Tkinter, updating directly from thread can be risky, but usually safe for simple label config 
                # if thread collisions are rare. Ideally use .after, but for simplicity here:
                self.after(0, lambda: self._set_image(ctk_img))
        except Exception as e:
            print(f"[UI] Failed to load image for {self.name}: {e}")

    def _set_image(self, img):
        self.image_label.configure(image=img, text="")
        self.image = img # Keep reference!


class DropsFrame(ctk.CTkFrame):
    def __init__(self, master, config_manager, api_client, **kwargs):
        print(f"!!! LOADING NEW DROPS FRAME (REDESIGN) !!! FROM: {__file__}")
        super().__init__(master, **kwargs)
        self.config = config_manager
        self.api = api_client
        
        # Grid layout
        self.grid_columnconfigure(0, weight=1) # Priority List
        self.grid_columnconfigure(1, weight=2) # Active Campaigns - giving it more space now
        self.grid_rowconfigure(0, weight=1)
        
        # --- Left Side: Priority Lists (Preserved) ---
        self.priority_frame = ctk.CTkFrame(self)
        self.priority_frame.grid(row=0, column=0, sticky="nsew", padx=10, pady=10)
        self.priority_frame.grid_rowconfigure(1, weight=1)
        self.priority_frame.grid_rowconfigure(3, weight=1)
        
        # Game Priority Section
        ctk.CTkLabel(self.priority_frame, text="Game Priority", font=("Roboto", 16, "bold")).grid(row=0, column=0, pady=5, padx=10, sticky="w")
        
        self.game_entry = ctk.CTkEntry(self.priority_frame, placeholder_text="Search Game...")
        self.game_entry.grid(row=0, column=1, pady=5, padx=5)
        self.game_entry.bind("<KeyRelease>", self.on_game_search)
        
        ctk.CTkButton(self.priority_frame, text="+", width=30, command=self.add_game_from_entry).grid(row=0, column=2, pady=5, padx=5)
        
        # Search Results Overlay
        self.search_results_frame = ctk.CTkScrollableFrame(self.priority_frame, height=150, label_text="Results")
        # Initially hidden
        
        self.game_list_frame = ctk.CTkScrollableFrame(self.priority_frame, label_text="High Priority")
        self.game_list_frame.grid(row=1, column=0, columnspan=3, sticky="nsew", padx=5, pady=5)
        
        # Streamer Priority Section
        ctk.CTkLabel(self.priority_frame, text="Streamer Priority", font=("Roboto", 16, "bold")).grid(row=2, column=0, pady=10, padx=10, sticky="w")
        
        self.streamer_entry = ctk.CTkEntry(self.priority_frame, placeholder_text="Enter Streamer Login")
        self.streamer_entry.grid(row=2, column=1, pady=5, padx=5)
        ctk.CTkButton(self.priority_frame, text="+", width=30, command=self.add_streamer).grid(row=2, column=2, pady=5, padx=5)
        
        self.streamer_list_frame = ctk.CTkScrollableFrame(self.priority_frame, label_text="Preferred Streamers")
        self.streamer_list_frame.grid(row=3, column=0, columnspan=3, sticky="nsew", padx=5, pady=5)

        # --- Right Side: Active Inventory / Drops (Redesigned) ---
        self.campaigns_frame = ctk.CTkFrame(self)
        self.campaigns_frame.grid(row=0, column=1, sticky="nsew", padx=10, pady=10)
        self.campaigns_frame.grid_rowconfigure(1, weight=1)
        self.campaigns_frame.grid_columnconfigure(0, weight=1)
        
        ctk.CTkLabel(self.campaigns_frame, text="My Drops Inventory", font=("Roboto", 16, "bold")).grid(row=0, column=0, pady=10, padx=10, sticky="w")
        ctk.CTkButton(self.campaigns_frame, text="Refresh", width=80, command=self.refresh_campaigns).grid(row=0, column=1, pady=10, padx=10, sticky="e")
        
        self.drops_scroll = ctk.CTkScrollableFrame(self.campaigns_frame)
        self.drops_scroll.grid(row=1, column=0, columnspan=2, sticky="nsew", padx=5, pady=5)
        self.drops_scroll.grid_columnconfigure(0, weight=1)
        
        # Load initial data
        self.load_priorities()
        # Auto refresh drops on load?
        # self.refresh_campaigns() 
        
    def on_game_search(self, event):
        query = self.game_entry.get().strip()
        if len(query) < 2:
            if self.search_results_frame.winfo_ismapped():
                self.search_results_frame.grid_forget()
                self.game_list_frame.grid(row=1, column=0, columnspan=3, sticky="nsew", padx=5, pady=5) # Restore list
            return

        threading.Thread(target=self._perform_search, args=(query,), daemon=True).start()

    def _perform_search(self, query):
        try:
            results = self.api.search_games(query)
            # Schedule UI update on main thread
            self.after(0, lambda: self._update_search_results(results))
        except Exception as e:
            print(f"Search Error: {e}")

    def _update_search_results(self, results):
        if results:
            if not self.search_results_frame.winfo_ismapped():
                 self.game_list_frame.grid_forget()
                 self.search_results_frame.grid(row=1, column=0, columnspan=3, sticky="nsew", padx=5, pady=5)
            
            # Clear existing
            for w in self.search_results_frame.winfo_children(): w.destroy()
            
            # Parse SearchFor response
            games_data = results.get("data", {}).get("searchFor", {}).get("games", {}).get("edges", [])
            count = 0
            for edge in games_data:
                node = edge.get("item")
                if node and "displayName" in node: 
                    name = node["displayName"]
                    btn = ctk.CTkButton(self.search_results_frame, text=name, anchor="w", fg_color="transparent", border_width=1, 
                                      command=lambda n=name: self.select_game_search(n))
                    btn.pack(fill="x", pady=1)
                    count += 1
                    if count >= 10: break
        else:
            pass

    def select_game_search(self, game_name):
        self.game_entry.delete(0, "end")
        self.game_entry.insert(0, game_name)
        self.add_game_from_entry()
        # Hide search, restore priority list
        self.search_results_frame.grid_forget()
        self.game_list_frame.grid(row=1, column=0, columnspan=3, sticky="nsew", padx=5, pady=5)

    def add_game_from_entry(self):
        game = self.game_entry.get().strip()
        if game:
            current = self.config.get("priorities", {}).get("games", [])
            if game not in current:
                current.append(game)
                self.config.set("priorities", {"games": current, "streamers": self.config.get("priorities", {}).get("streamers", [])})
                self.game_entry.delete(0, "end")
                self.load_priorities()
                if self.search_results_frame.winfo_ismapped():
                    self.search_results_frame.grid_forget()
                    self.game_list_frame.grid(row=1, column=0, columnspan=3, sticky="nsew", padx=5, pady=5)

    def add_game(self):
        # Legacy method kept for button compatibility
        self.add_game_from_entry()
                
    def add_streamer(self):
        streamer = self.streamer_entry.get().strip()
        if streamer:
            current = self.config.get("priorities", {}).get("streamers", [])
            if streamer not in current:
                current.append(streamer)
                self.config.set("priorities", {"games": self.config.get("priorities", {}).get("games", []), "streamers": current})
                self.streamer_entry.delete(0, "end")
                self.load_priorities()

    def remove_game(self, game_name):
        current = self.config.get("priorities", {}).get("games", [])
        if game_name in current:
            current.remove(game_name)
            self.config.set("priorities", {"games": current, "streamers": self.config.get("priorities", {}).get("streamers", [])})
            self.load_priorities()

    def remove_streamer(self, streamer_name):
        current = self.config.get("priorities", {}).get("streamers", [])
        if streamer_name in current:
            current.remove(streamer_name)
            self.config.set("priorities", {"games": self.config.get("priorities", {}).get("games", []), "streamers": current})
            self.load_priorities()

    def move_game_up(self, game_name):
        current = self.config.get("priorities", {}).get("games", [])
        if game_name in current:
            idx = current.index(game_name)
            if idx > 0:
                current[idx], current[idx-1] = current[idx-1], current[idx]
                self.config.set("priorities", {"games": current, "streamers": self.config.get("priorities", {}).get("streamers", [])})
                self.load_priorities()

    def move_game_down(self, game_name):
        current = self.config.get("priorities", {}).get("games", [])
        if game_name in current:
            idx = current.index(game_name)
            if idx < len(current) - 1:
                current[idx], current[idx+1] = current[idx+1], current[idx]
                self.config.set("priorities", {"games": current, "streamers": self.config.get("priorities", {}).get("streamers", [])})
                self.load_priorities()

    def load_priorities(self):
        # Clear lists
        for widget in self.game_list_frame.winfo_children(): widget.destroy()
        for widget in self.streamer_list_frame.winfo_children(): widget.destroy()
        
        games = self.config.get("priorities", {}).get("games", [])
        for i, game in enumerate(games):
            f = ctk.CTkFrame(self.game_list_frame, fg_color="transparent")
            f.pack(fill="x", pady=2)
            
            # Reordering Buttons
            if i > 0:
                ctk.CTkButton(f, text="▲", width=20, fg_color="gray40", command=lambda g=game: self.move_game_up(g)).pack(side="left", padx=2)
            else:
                ctk.CTkLabel(f, text=" ", width=20).pack(side="left", padx=2) # Spacer
                
            if i < len(games) - 1:
                ctk.CTkButton(f, text="▼", width=20, fg_color="gray40", command=lambda g=game: self.move_game_down(g)).pack(side="left", padx=2)
            else:
                ctk.CTkLabel(f, text=" ", width=20).pack(side="left", padx=2) # Spacer

            ctk.CTkLabel(f, text=game).pack(side="left", padx=5)
            ctk.CTkButton(f, text="X", width=20, fg_color="red", command=lambda g=game: self.remove_game(g)).pack(side="right", padx=5)
            
        streamers = self.config.get("priorities", {}).get("streamers", [])
        for s in streamers:
            f = ctk.CTkFrame(self.streamer_list_frame, fg_color="transparent")
            f.pack(fill="x", pady=2)
            ctk.CTkLabel(f, text=s).pack(side="left", padx=5)
            ctk.CTkButton(f, text="X", width=20, fg_color="red", command=lambda x=s: self.remove_streamer(x)).pack(side="right", padx=5)

    def refresh_campaigns(self):
        # --- REDESIGNED from Inventory Data ---
        for widget in self.drops_scroll.winfo_children(): widget.destroy()
        
        try:
            # Call 'get_drops_campaigns' which now calls 'get_inventory'
            inv_data = self.api.get_drops_campaigns()
            if inv_data:
                user_data = inv_data.get("data", {}).get("currentUser", {})
                inventory_data = user_data.get("inventory", {})
                campaigns = inventory_data.get("dropCampaignsInProgress") or []
                
                # TRACKED GAMES SET
                tracked_names = set()
                
                # Iterate campaigns (Inventory)
                if campaigns:
                    for camp in campaigns:
                        game = camp.get("game", {})
                        game_name = game.get("name") or game.get("displayName") or "Unknown Game"
                        tracked_names.add(game_name.lower()) # details
                        tracked_names.add(game.get("displayName", "").lower())
                        
                        # ... existing campaign card logic ...
                        # Header for Game
                        header = ctk.CTkLabel(self.drops_scroll, text=f"{game_name} Drops", font=("Roboto", 16, "bold"), fg_color="gray20", corner_radius=6)
                        header.pack(fill="x", pady=(15, 5), padx=5)
                        
                        drops = camp.get("timeBasedDrops", [])
                        for drop in drops:
                            card = DropCard(self.drops_scroll, drop_data=drop)
                            card.pack(fill="x", pady=2, padx=10)

                # --- NEW: Check Priority Games NOT in Inventory ---
                priority_games = self.config.get("priorities", {}).get("games", [])
                for p_game in priority_games:
                    # quick fuzzy check
                    if p_game.lower() in tracked_names:
                        continue 
                        
                    # Not in inventory, let's check if it's active!
                    self.check_priority_status(p_game)
                    
            else:
                 ctk.CTkLabel(self.drops_scroll, text="Failed to fetch inventory.").pack(pady=10)
        except Exception as e:
             ctk.CTkLabel(self.drops_scroll, text=f"Error: {str(e)}").pack(pady=10)

    def check_priority_status(self, game_name):
        # We need to run this in a thread to avoid blocking
        threading.Thread(target=self._check_game_status_thread, args=(game_name,), daemon=True).start()

    def _check_game_status_thread(self, game_name):
        print(f"[UI DEBUG] checking status for: {game_name}")
        
        # 1. Generate Slug using robust method
        slug = self.api.get_slug(game_name)
        disp_name = game_name
        
        print(f"[UI DEBUG] Generated Slug for {game_name}: {slug}")

        if not slug:
            print(f"[UI DEBUG] No slug found for {game_name}")
            return

        # 2. Get Live Streams
        streams = self.api.get_live_streams_by_slug(slug)
        found_details = None
        
        if streams:
             edges = streams.get("data", {}).get("game", {}).get("streams", {}).get("edges", [])
             print(f"[UI DEBUG] Found {len(edges)} live streams for {slug}")
             
             for edge in edges:
                 node = edge.get("node", {})
                 
                 # Check 'freeformTags' for explicit text (or legacy system tags)
                 ff_tags = node.get("freeformTags", [])
                 tags = node.get("tags", [])
                 
                 has_drops_tag = False
                 if any(t.get("name") == "DropsEnabled" for t in ff_tags): has_drops_tag = True
                 elif any(t.get("id") == "c2542d6d-cd10-4532-919b-3d19f30a768b" for t in tags): has_drops_tag = True
                 
                 if has_drops_tag:
                     # FOUND ACTIVE STREAM!
                     # Now let's try to get the details of the drop from this channel
                     broadcaster_id = node.get("broadcaster", {}).get("id")
                     if broadcaster_id:
                         print(f"[UI DEBUG] Fetching drops details from channel {broadcaster_id}...")
                         details = self.api.get_available_drops(broadcaster_id)
                         if details:
                             # Extract the first relevant campaign/drop
                             # Structure: data -> channel -> viewerDropCampaigns 
                             campaigns = details.get("data", {}).get("channel", {}).get("viewerDropCampaigns", [])
                             
                             for camp in campaigns:
                                 # Game object in this query might not have slug, only name/id
                                 g_name = camp.get("game", {}).get("name")
                                 c_slug = self.api.get_slug(g_name) if g_name else None
                                 
                                 # Match against our target slug
                                 if c_slug == slug:
                                     found_details = camp
                                     break
                                     
                         if found_details:
                             break # Stop strictly if we found what we needed
                         else:
                             print(f"[UI DEBUG] Channel {broadcaster_id} had drops enabled but no matching campaign. Trying next stream...")
                             
                 # Continue loop if not found yet
        
        # 3. Update UI
        if found_details:
             self.after(0, lambda: self._add_status_card(disp_name, found_details))

    def _add_status_card(self, game_name, campaign_data):
        f = ctk.CTkFrame(self.drops_scroll)
        f.pack(fill="x", pady=5, padx=5)
        
        # Header
        ctk.CTkLabel(f, text=f"{game_name} (Not Started)", font=("Roboto", 14, "bold")).pack(anchor="w", padx=10, pady=(5,2))
        
        # We can reuse DropCard logic if we extract a "drop" object from the campaign
        # Campaign has 'timeBasedDrops' usually
        drops = campaign_data.get("timeBasedDrops", [])
        if drops:
            # Create a simplified card for the first drop to show what's available
            first_drop = drops[0]
            
            # The DropCard expects 'self' data for progress, which we don't have (since it's not in inventory).
            # We'll mock it or modify DropCard to handle missing 'self'.
            # Let's Mock it to 0 progress.
            if "self" not in first_drop:
                first_drop["self"] = {"currentMinutesWatched": 0, "isClaimed": False}
                first_drop["requiredMinutesWatched"] = first_drop.get("requiredMinutesWatched", 60) # Default if missing
            
            # Use DropCard but maybe style it finding?
            card = DropCard(f, drop_data=first_drop, fg_color="transparent")
            card.pack(fill="x", padx=5, pady=5)
            
            ctk.CTkLabel(f, text="Add to Miner to start!", text_color="cyan", font=("Roboto", 10)).pack(anchor="e", padx=10, pady=(0,5))
        else:
            ctk.CTkLabel(f, text="Drops found but no details available.", text_color="yellow").pack(anchor="w", padx=10)


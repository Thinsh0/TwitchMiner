import customtkinter as ctk
from PIL import Image
import requests
from io import BytesIO
import threading

# Re-implementing a simplified DropCard for Dashboard usage to ensure consistent image loading
class DashboardDropCard(ctk.CTkFrame):
    def __init__(self, master, image_url, **kwargs):
        super().__init__(master, **kwargs)
        self.image_url = image_url
        self.image_label = ctk.CTkLabel(self, text="...", width=100, height=100)
        self.image_label.pack(fill="both", expand=True)
        
        self.load_image()

    def load_image(self):
        # print(f"[DashboardCard] Requesting Image: {self.image_url}")
        if not self.image_url:
            self.image_label.configure(text="No Image")
            return

        def _load():
            try:
                # print(f"[DashboardCard] Downloading...")
                response = requests.get(self.image_url, timeout=5)
                # print(f"[DashboardCard] Downloaded. Status: {response.status_code}, Len: {len(response.content)}")
                img_data = BytesIO(response.content)
                img = Image.open(img_data)
                ctk_img = ctk.CTkImage(light_image=img, dark_image=img, size=(100, 100))
                
                # Update on main thread
                def _update_ui():
                    self.image_label.configure(image=ctk_img, text="")
                    # print("[DashboardCard] Image Configured on Label.")

                self.image_label.after(0, _update_ui)
            except Exception as e:
                print(f"[DashboardCard] Error: {e}")
                self.image_label.after(0, lambda: self.image_label.configure(text=f"Err: {e}"))
        
        threading.Thread(target=_load, daemon=True).start()

class AsyncImageLabel(ctk.CTkLabel):
    def __init__(self, master, size=(50, 50), default_text="", **kwargs):
        super().__init__(master, text=default_text, width=size[0], height=size[1], **kwargs)
        self.size = size
        self.current_url = None
        self._image_ref = None # Keep reference

    def set_image_url(self, url):
        if url == self.current_url: return
        self.current_url = url
        
        if not url:
            self.configure(image=None, text="?")
            self._image_ref = None
            return

        def _load():
            try:
                response = requests.get(url, timeout=5)
                if response.status_code == 200:
                    img_data = BytesIO(response.content)
                    img = Image.open(img_data)
                    ctk_img = ctk.CTkImage(light_image=img, dark_image=img, size=self.size)
                    
                    def _update():
                        if self.current_url == url: # Check for race condition
                            self.configure(image=ctk_img, text="")
                            self._image_ref = ctk_img
                    
                    self.after(0, _update)
            except Exception as e:
                print(f"[AsyncImage] Error loading {url}: {e}")

        threading.Thread(target=_load, daemon=True).start()

class DashboardFrame(ctk.CTkFrame):
    def __init__(self, master, miner):
        super().__init__(master)
        self.miner = miner
        # We will use the new card for image display
        self.current_card = None 
        self.last_image_url = None
        
        self.grid_columnconfigure(0, weight=1)
        self.grid_columnconfigure(1, weight=1)
        
        # Header
        self.label_title = ctk.CTkLabel(self, text="Dashboard", font=ctk.CTkFont(size=24, weight="bold"))
        self.label_title.grid(row=0, column=0, columnspan=2, padx=20, pady=20, sticky="w")
        
        # Status Card
        self.status_frame = ctk.CTkFrame(self)
        self.status_frame.grid(row=1, column=0, columnspan=2, padx=20, pady=10, sticky="nsew")
        self.status_frame.grid_columnconfigure(1, weight=1) # Text column expands
        
        # Image Container (Left)
        self.image_container = ctk.CTkFrame(self.status_frame, width=120, height=120, fg_color="transparent")
        self.image_container.grid(row=0, column=0, rowspan=4, padx=15, pady=15)
        self.image_container.pack_propagate(False) # Fixed size
        
        # Info (Right)
        self.lbl_status = ctk.CTkLabel(self.status_frame, text="Status: Idle", font=ctk.CTkFont(size=16, weight="bold"))
        self.lbl_status.grid(row=0, column=1, padx=10, pady=(15, 5), sticky="w")
        
        self.lbl_streamer_frame = ctk.CTkFrame(self.status_frame, fg_color="transparent")
        self.lbl_streamer_frame.grid(row=1, column=1, padx=10, pady=2, sticky="w")
        
        self.avatar = AsyncImageLabel(self.lbl_streamer_frame, size=(40, 40), default_text="?")
        self.avatar.pack(side="left", padx=(0, 10))
        
        self.lbl_streamer = ctk.CTkLabel(self.lbl_streamer_frame, text="Streamer: None")
        self.lbl_streamer.pack(side="left")
        
        self.lbl_drop_name = ctk.CTkLabel(self.status_frame, text="Drop: None")
        self.lbl_drop_name.grid(row=2, column=1, padx=10, pady=2, sticky="w")

        # Progress Bars (Right side, below info)
        self.progress_frame = ctk.CTkFrame(self.status_frame, fg_color="transparent")
        self.progress_frame.grid(row=3, column=1, padx=10, pady=10, sticky="ew")
        
        # Drop Progress
        self.lbl_drop_title = ctk.CTkLabel(self.progress_frame, text="Current Drop", font=ctk.CTkFont(size=12))
        self.lbl_drop_title.pack(anchor="w")
        self.progress_bar = ctk.CTkProgressBar(self.progress_frame, progress_color="#9146FF")
        self.progress_bar.pack(pady=5, fill="x")
        self.progress_bar.set(0)
        self.lbl_drop_pct = ctk.CTkLabel(self.progress_frame, text="0%", font=ctk.CTkFont(size=10))
        self.lbl_drop_pct.pack(pady=(0,5))

        # Campaign Progress
        self.lbl_camp_title = ctk.CTkLabel(self.progress_frame, text="Campaign Total", font=ctk.CTkFont(size=12))
        self.lbl_camp_title.pack(anchor="w", pady=(10,0))
        self.campaign_bar = ctk.CTkProgressBar(self.progress_frame, progress_color="#4691FF")
        self.campaign_bar.pack(pady=5, fill="x")
        self.campaign_bar.set(0)
        self.lbl_camp_pct = ctk.CTkLabel(self.progress_frame, text="0%", font=ctk.CTkFont(size=10))
        self.lbl_camp_pct.pack(pady=(0,5))


        # Control Buttons
        self.btn_start = ctk.CTkButton(self, text="Start Mining", command=self.start_miner)
        self.btn_start.grid(row=2, column=0, padx=20, pady=20)
        
        self.btn_stop = ctk.CTkButton(self, text="Stop Mining", command=self.stop_miner, fg_color="red", hover_color="darkred")
        self.btn_stop.grid(row=2, column=1, padx=20, pady=20)

        self._setup_bonus_summary()

        # Update Loop
        self.update_dashboard()
        
        # Auto-Start Logic
        if self.miner.config.get("settings", {}).get("auto_start_mining", False):
            if self.miner.config.get_auth_token():
                print("[Dashboard] Auto-Starting Miner...")
                self.after(2000, self.start_miner)
            else:
                 print("[Dashboard] Auto-Start skipped: Not Logged In")

    def start_miner(self):
        self.miner.start()
        self.lbl_status.configure(text="Status: Starting...")

    def stop_miner(self):
        self.miner.stop()
        self.lbl_status.configure(text="Status: Stopping...")

    def update_dashboard(self):
        status_data = self.miner.get_status()
        
        self.lbl_status.configure(text=f"Status: {status_data['status']}")
        self.lbl_streamer.configure(text=f"Streamer: {status_data['streamer'] or 'None'}")
        self.avatar.set_image_url(status_data.get('streamer_image_url'))
        self.lbl_drop_name.configure(text=f"Drop: {status_data['drop'] or 'None'}")
        
        # Drop Progress
        try:
            val = float(status_data['progress']) / 100.0
            self.progress_bar.set(val)
            self.lbl_drop_pct.configure(text=f"{int(status_data['progress'])}%")
        except:
            self.progress_bar.set(0)
            self.lbl_drop_pct.configure(text="0%")
            
        # Campaign Progress
        try:
            c_val = float(status_data.get('campaign_progress', 0)) / 100.0
            self.campaign_bar.set(c_val)
            self.lbl_camp_pct.configure(text=f"{int(status_data.get('campaign_progress', 0))}%")
        except:
            self.campaign_bar.set(0)
            self.lbl_camp_pct.configure(text="0%")

        # Image Handling via Card
        img_url = status_data.get('image_url')
        if img_url != self.last_image_url:
            self.last_image_url = img_url
            
            # Clear old
            for w in self.image_container.winfo_children(): w.destroy()
            
            # Create new card
            if img_url:
                self.current_card = DashboardDropCard(self.image_container, img_url)
                self.current_card.pack(fill="both", expand=True)

        self.after(1000, self.update_dashboard)

        self.after(1000, self.update_dashboard)

    def _setup_bonus_summary(self):
        # NEW SECTION: Bonus & Points (List View)
        self.bonus_frame = ctk.CTkFrame(self)
        self.bonus_frame.grid(row=4, column=0, columnspan=2, padx=20, pady=10, sticky="nsew")
        
        # Header
        ctk.CTkLabel(self.bonus_frame, text="Active Farming & Streaks", font=ctk.CTkFont(size=14, weight="bold")).pack(pady=5)
        
        # Scrollable List
        self.bonus_list = ctk.CTkScrollableFrame(self.bonus_frame, height=150, label_text="Channels")
        self.bonus_list.pack(fill="both", expand=True, padx=10, pady=5)
        
        # We will populate this dynamically in update_dashboard

    def update_dashboard(self):
        status_data = self.miner.get_status()
        
        self.lbl_status.configure(text=f"Status: {status_data['status']}")
        self.lbl_streamer.configure(text=f"Streamer: {status_data['streamer'] or 'None'}")
        self.avatar.set_image_url(status_data.get('streamer_image_url'))
        self.lbl_drop_name.configure(text=f"Drop: {status_data['drop'] or 'None'}")
        
        # Add Points Display in Status Card (Drop Box)
        # We need to add a label for this if not exists, or append to one.
        # Let's create `self.lbl_drop_points` in __init__ properly, but here we can try to hack it in if missing, or better, reuse lbl_drop_name's grid row to add another label?
        # Actually I should have added it in __init__. Let's modify __init__ separately or just check if it exists.
        if not hasattr(self, "lbl_drop_points"):
             self.lbl_drop_points = ctk.CTkLabel(self.status_frame, text="Points: --", text_color="#00E5FF")
             self.lbl_drop_points.grid(row=2, column=2, padx=10, sticky="e") # Right of Drop name? 
             # Wait, grid layout might be tricky. Let's put it below Drop Name.
             self.lbl_drop_points.grid(row=3, column=1, padx=10, pady=(0,20), sticky="nw") # Overlapping progress bar?
             # Let's re-grid progress bar to row 4
             self.progress_frame.grid(row=4, column=1, padx=10, pady=10, sticky="ew")
        
        pts = status_data.get("current_points", "--")
        if pts == 0 or pts == "0": pts = "--"
        self.lbl_drop_points.configure(text=f"Points: {pts}")

        # Drop Progress
        try:
            val = float(status_data['progress']) / 100.0
            self.progress_bar.set(val)
            self.lbl_drop_pct.configure(text=f"{int(status_data['progress'])}%")
        except:
            self.progress_bar.set(0)
            self.lbl_drop_pct.configure(text="0%")
            
        # Campaign Progress
        try:
            c_val = float(status_data.get('campaign_progress', 0)) / 100.0
            self.campaign_bar.set(c_val)
            self.lbl_camp_pct.configure(text=f"{int(status_data.get('campaign_progress', 0))}%")
        except:
            self.campaign_bar.set(0)
            self.lbl_camp_pct.configure(text="0%")

        # Image Handling via Card
        img_url = status_data.get('image_url')
        if img_url != self.last_image_url:
            self.last_image_url = img_url
            for w in self.image_container.winfo_children(): w.destroy()
            if img_url:
                self.current_card = DashboardDropCard(self.image_container, img_url)
                self.current_card.pack(fill="both", expand=True)

        # --- UPDATE BONUS LIST ---
        # Clear list first (inefficient but safe for now, or optimise later)
        if hasattr(self, "bonus_list"):
            for w in self.bonus_list.winfo_children(): w.destroy()
            
            # Access BonusManager
            if hasattr(self.miner, "bonus_manager"):
                bm = self.miner.bonus_manager
                
                # 1. Points Farming Stats
                if bm.points.is_running:
                     for ch, data in bm.points.stats.items():
                         f = ctk.CTkFrame(self.bonus_list, fg_color="transparent")
                         f.pack(fill="x", pady=1)
                         ctk.CTkLabel(f, text=f"[Farm] {ch}", font=("Roboto", 12, "bold"), width=100, anchor="w").pack(side="left")
                         ctk.CTkLabel(f, text=f"{data.get('points', '--')} pts", text_color="#00E5FF", width=60).pack(side="left")
                         ctk.CTkLabel(f, text=data.get('status', 'Idle'), text_color="gray").pack(side="left", padx=5)
                
                # 2. Daily Streaks
                # We don't have a live stats dict for daily streaks easily exposed unless we check config or add one.
                # But we can check 'current_channel' if sequential.
                if bm.daily.is_running:
                    if hasattr(bm.daily, "current_channel") and bm.daily.current_channel:
                         f = ctk.CTkFrame(self.bonus_list, fg_color="transparent")
                         f.pack(fill="x", pady=1)
                         ctk.CTkLabel(f, text=f"[Streak] {bm.daily.current_channel}", font=("Roboto", 12, "bold"), text_color="orange", width=100, anchor="w").pack(side="left")
                         ctk.CTkLabel(f, text="Watching...", text_color="yellow").pack(side="left", padx=5)
            else:
                 ctk.CTkLabel(self.bonus_list, text="Bonus Manager not active or connected.", text_color="gray").pack()

        self.after(1000, self.update_dashboard)

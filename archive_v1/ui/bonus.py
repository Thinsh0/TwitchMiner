import customtkinter as ctk
import threading
from core.bonus import BonusManager
from ui.dashboard import AsyncImageLabel 

class BonusFrame(ctk.CTkFrame):
    def __init__(self, master, config_manager, api_client, miner):
        super().__init__(master)
        self.config = config_manager
        self.gql = api_client
        self.manager = BonusManager(config_manager, api_client, miner)
        if miner:
             miner.bonus_manager = self.manager
        
        # Title
        ctk.CTkLabel(self, text="Farming & Bonuses", font=("Roboto", 20, "bold")).pack(pady=10)
        
        # Main Layout: Side-by-Side
        self.grid_columnconfigure(0, weight=1)
        self.grid_columnconfigure(1, weight=1)
        self.grid_rowconfigure(1, weight=1)

        # --- LEFT: Daily Streaks ---
        self.left_frame = ctk.CTkFrame(self)
        self.left_frame.pack(side="left", fill="both", expand=True, padx=10, pady=10)
        self._setup_streaks_ui(self.left_frame)

        # --- RIGHT: Points Mining ---
        self.right_frame = ctk.CTkFrame(self)
        self.right_frame.pack(side="right", fill="both", expand=True, padx=10, pady=10)
        self._setup_points_ui(self.right_frame)

    def _setup_streaks_ui(self, parent):
        ctk.CTkLabel(parent, text="Daily Streaks", font=("Roboto", 16, "bold")).pack(pady=10)
        
        # Controls
        ctrl = ctk.CTkFrame(parent)
        ctrl.pack(fill="x", padx=5, pady=5)
        
        self.entry_streak = ctk.CTkEntry(ctrl, placeholder_text="Channel Name", width=120)
        self.entry_streak.pack(side="left", padx=5)
        ctk.CTkButton(ctrl, text="+", width=30, command=self.add_streak_channel).pack(side="left", padx=5)
        
        self.btn_run_streaks = ctk.CTkButton(ctrl, text="Start Daily", fg_color="green", width=100, command=self.toggle_streaks)
        self.btn_run_streaks.pack(side="right", padx=5)
        
        # List
        self.streak_list = ctk.CTkScrollableFrame(parent, label_text="Channels")
        self.streak_list.pack(fill="both", expand=True, padx=5, pady=5)
        
        self.refresh_streak_list()

    def _setup_points_ui(self, parent):
        ctk.CTkLabel(parent, text="Points Farming", font=("Roboto", 16, "bold")).pack(pady=10)
        
        # Controls
        ctrl = ctk.CTkFrame(parent)
        ctrl.pack(fill="x", padx=5, pady=5)
        
        self.entry_points = ctk.CTkEntry(ctrl, placeholder_text="Channel Name", width=120)
        self.entry_points.pack(side="left", padx=5)
        ctk.CTkButton(ctrl, text="+", width=30, command=self.add_points_channel).pack(side="left", padx=5)
        
        self.btn_run_points = ctk.CTkButton(ctrl, text="Start Farming", fg_color="purple", width=100, command=self.toggle_points)
        self.btn_run_points.pack(side="right", padx=5)
        
        # Check initial state (Auto-Start)
        if self.manager.points.is_running:
            self.btn_run_points.configure(text="Stop Farming", fg_color="red")
        
        # List
        self.points_list = ctk.CTkScrollableFrame(parent, label_text="Targets")
        self.points_list.pack(fill="both", expand=True, padx=5, pady=5)
        
        self.refresh_points_list()
        
        # Update Loop
        self.after(2000, self.update_points_ui)

    # --- LOGIC ---

    def add_streak_channel(self):
        name = self.entry_streak.get().strip().lower()
        if name:
            bonus = self.config.get("bonus", {})
            channels = bonus.get("daily_streaks", {}).get("channels", [])
            if name not in channels:
                channels.append(name)
                # Ensure structure exists
                if "daily_streaks" not in bonus: bonus["daily_streaks"] = {}
                bonus["daily_streaks"]["channels"] = channels
                self.config.set("bonus", bonus)
                self.refresh_streak_list()
            self.entry_streak.delete(0, "end")

    def remove_streak_channel(self, name):
         bonus = self.config.get("bonus", {})
         channels = bonus.get("daily_streaks", {}).get("channels", [])
         if name in channels:
             channels.remove(name)
             bonus["daily_streaks"]["channels"] = channels
             self.config.set("bonus", bonus)
             self.refresh_streak_list()

    def add_points_channel(self):
        name = self.entry_points.get().strip().lower()
        if name:
            bonus = self.config.get("bonus", {})
            channels = bonus.get("points_farming", {}).get("channels", [])
            if name not in channels:
                channels.append(name)
                if "points_farming" not in bonus: bonus["points_farming"] = {}
                bonus["points_farming"]["channels"] = channels
                self.config.set("bonus", bonus)
                self.refresh_points_list()
            self.entry_points.delete(0, "end")

    def remove_points_channel(self, name):
         bonus = self.config.get("bonus", {})
         channels = bonus.get("points_farming", {}).get("channels", [])
         if name in channels:
             channels.remove(name)
             bonus["points_farming"]["channels"] = channels
             self.config.set("bonus", bonus)
             self.refresh_points_list()

    def refresh_streak_list(self):
        for w in self.streak_list.winfo_children(): w.destroy()
        
        bonus = self.config.get("bonus", {})
        daily_cfg = bonus.get("daily_streaks", {})
        channels = daily_cfg.get("channels", [])
        history = daily_cfg.get("history", {})
        values = daily_cfg.get("values", {})
        
        import datetime
        today = datetime.date.today().isoformat()
        
        for ch in channels:
            f = ctk.CTkFrame(self.streak_list)
            f.pack(fill="x", pady=2)
            
            ctk.CTkLabel(f, text=ch, font=("Roboto", 13)).pack(side="left", padx=5)
            
            # Value
            val = values.get(ch, "?")
            ctk.CTkLabel(f, text=f"Streak: {val}", text_color="#AABBCC", font=("Roboto", 12)).pack(side="left", padx=10)
            
            last_run = history.get(ch)
            status_txt = "Done" if last_run == today else "Pending"
            status_col = "green" if last_run == today else "orange"
            
            ctk.CTkLabel(f, text=status_txt, text_color=status_col, font=("Roboto", 12, "bold")).pack(side="left", padx=10)
            
            ctk.CTkButton(f, text="x", width=20, fg_color="transparent", text_color="red", hover_color="#440000", command=lambda c=ch: self.remove_streak_channel(c)).pack(side="right", padx=2)

    def refresh_points_list(self):
        for w in self.points_list.winfo_children(): w.destroy()
        
        bonus = self.config.get("bonus", {})
        channels = bonus.get("points_farming", {}).get("channels", [])
        
        self.points_labels = {} 
        
        for ch in channels:
            f = ctk.CTkFrame(self.points_list)
            f.pack(fill="x", pady=2)
            
            ctk.CTkLabel(f, text=ch, font=("Roboto", 13)).pack(side="left", padx=5)
            
            lbl_pts = ctk.CTkLabel(f, text="--", text_color="#00E5FF", font=("Roboto", 12, "bold"))
            lbl_pts.pack(side="left", padx=10)
            
            lbl_stat = ctk.CTkLabel(f, text="Idle", text_color="gray", font=("Roboto", 11))
            lbl_stat.pack(side="left", padx=5)
            
            ctk.CTkButton(f, text="x", width=20, fg_color="transparent", text_color="red", hover_color="#440000", command=lambda c=ch: self.remove_points_channel(c)).pack(side="right", padx=2)
            
            self.points_labels[ch] = (lbl_pts, lbl_stat)

    def toggle_streaks(self):
        if self.manager.daily.is_running:
             self.manager.daily.is_running = False
             self.btn_run_streaks.configure(text="Stopping...", fg_color="gray")
        else:
             threading.Thread(target=self._run_daily_thread, daemon=True).start()
             self.btn_run_streaks.configure(text="Running...", fg_color="red")
             
    def _run_daily_thread(self):
        self.manager.daily.run_daily_routine()
        self.after(0, lambda: self.btn_run_streaks.configure(text="Start Daily", fg_color="green"))
        self.after(0, self.refresh_streak_list)

    def toggle_points(self):
        if self.manager.points.is_running:
            self.manager.points.stop_farming()
            self.btn_run_points.configure(text="Start Farming", fg_color="purple")
        else:
            self.manager.points.start_farming()
            self.btn_run_points.configure(text="Stop Farming", fg_color="red")

    def update_points_ui(self):
        if self.manager.points.is_running:
            stats = self.manager.points.stats
            for ch, data in stats.items():
                if ch in self.points_labels:
                    lbl_pts, lbl_stat = self.points_labels[ch]
                    lbl_pts.configure(text=f"{data['points']}")
                    lbl_stat.configure(text=data['status'])
        
        self.after(2000, self.update_points_ui)

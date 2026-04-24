import customtkinter as ctk
import threading
from core.streak_miner import StreakMiner

class StreakTab(ctk.CTkFrame):
    def __init__(self, master, config_manager, api_client):
        super().__init__(master)
        self.config = config_manager
        self.gql = api_client
        self.miner = StreakMiner(config_manager, api_client)
        
        # Grid Layout
        self.grid_columnconfigure(0, weight=1)
        self.grid_rowconfigure(1, weight=1) # List expands
        
        # Header / Controls
        self.ctrl_frame = ctk.CTkFrame(self)
        self.ctrl_frame.grid(row=0, column=0, padx=10, pady=10, sticky="ew")
        
        self.entry_channel = ctk.CTkEntry(self.ctrl_frame, placeholder_text="Channel Name")
        self.entry_channel.pack(side="left", padx=5, fill="x", expand=True)
        
        self.btn_add = ctk.CTkButton(self.ctrl_frame, text="Add", width=60, command=self.add_channel)
        self.btn_add.pack(side="left", padx=5)
        
        # Channel List
        self.list_frame = ctk.CTkScrollableFrame(self, label_text="Watch Streak Channels")
        self.list_frame.grid(row=1, column=0, padx=10, pady=5, sticky="nsew")
        
        # Bottom Controls
        self.bottom_frame = ctk.CTkFrame(self)
        self.bottom_frame.grid(row=2, column=0, padx=10, pady=10, sticky="ew")
        
        # Mode
        self.mode_var = ctk.StringVar(value=self.config.get("streaks", {}).get("mode", "sequential"))
        
        ctk.CTkLabel(self.bottom_frame, text="Mode:").pack(side="left", padx=5)
        ctk.CTkRadioButton(self.bottom_frame, text="Sequential", variable=self.mode_var, value="sequential", command=self.save_mode).pack(side="left", padx=5)
        ctk.CTkRadioButton(self.bottom_frame, text="Parallel", variable=self.mode_var, value="parallel", command=self.save_mode).pack(side="left", padx=5)
        
        # Start/Stop
        self.btn_start = ctk.CTkButton(self.bottom_frame, text="Start Streak", command=self.toggle_miner)
        self.btn_start.pack(side="right", padx=10)
        
        self.lbl_status = ctk.CTkLabel(self.bottom_frame, text="Status: Idle")
        self.lbl_status.pack(side="right", padx=10)

        # Init List
        self.refresh_list()
        
        # Update Status Loop
        self.update_status()

    def add_channel(self):
        name = self.entry_channel.get().strip()
        if name:
            streaks = self.config.get("streaks", {})
            channels = streaks.get("channels", [])
            if name not in channels:
                channels.append(name)
                streaks["channels"] = channels
                self.config.set("streaks", streaks)
                self.refresh_list()
            self.entry_channel.delete(0, "end")

    def remove_channel(self, name):
        streaks = self.config.get("streaks", {})
        channels = streaks.get("channels", [])
        if name in channels:
            channels.remove(name)
            streaks["channels"] = channels
            self.config.set("streaks", streaks)
            self.refresh_list()

    def save_mode(self):
        streaks = self.config.get("streaks", {})
        streaks["mode"] = self.mode_var.get()
        self.config.set("streaks", streaks)

    def refresh_list(self):
        for widget in self.list_frame.winfo_children():
            widget.destroy()
            
        channels = self.config.get("streaks", {}).get("channels", [])
        for ch in channels:
            row = ctk.CTkFrame(self.list_frame)
            row.pack(fill="x", pady=2)
            
            ctk.CTkLabel(row, text=ch).pack(side="left", padx=10)
            ctk.CTkButton(row, text="X", width=30, fg_color="red", command=lambda c=ch: self.remove_channel(c)).pack(side="right", padx=5)

    def toggle_miner(self):
        if self.miner.is_running:
            self.miner.stop()
            self.btn_start.configure(text="Start Streak", fg_color="#1f6aa5") # Default Blue
        else:
            self.miner.start()
            self.btn_start.configure(text="Stop Streak", fg_color="red")

    def update_status(self):
        if self.miner.is_running:
            self.lbl_status.configure(text=f"Status: {self.miner.status}")
        else:
            self.lbl_status.configure(text="Status: Idle")
        self.after(1000, self.update_status)

import customtkinter as ctk
from utils.localization import LocalizationManager
from utils.system import SystemManager

class SettingsFrame(ctk.CTkFrame):
    def __init__(self, master, config_manager, app_instance=None):
        super().__init__(master)
        self.config_manager = config_manager
        self.config = self.config_manager.config
        self.app = app_instance # Reference to main App for theme/restart
        
        self.loc = LocalizationManager(config_manager)
        
        # Main Layout
        self.main_frame = ctk.CTkScrollableFrame(self)
        self.main_frame.pack(fill="both", expand=True, padx=20, pady=20)
        
        self.title_label = ctk.CTkLabel(self.main_frame, text=self.loc.get("settings_title"), font=ctk.CTkFont(size=24, weight="bold"))
        self.title_label.pack(pady=20)
        
        # --- General Settings (Language, Priority Mode) ---
        self.create_section(self.loc.get("general"))
        
        # Language
        self.lang_var = ctk.StringVar(value=self.config.get("settings", {}).get("language", "EN"))
        ctk.CTkLabel(self.main_frame, text=self.loc.get("language")).pack(pady=(10,0))
        self.lang_combo = ctk.CTkComboBox(self.main_frame, values=["EN", "FR"], variable=self.lang_var, command=self.change_language)
        self.lang_combo.pack(pady=5)
        
        # Concurrency Mode
        ctk.CTkLabel(self.main_frame, text=self.loc.get("concurrency")).pack(pady=(10,0))
        modes = [self.loc.get("mode_drops_only"), self.loc.get("mode_smart"), self.loc.get("mode_parallel")]
        self.mode_var = ctk.StringVar(value=self.get_mode_label(self.config.get("settings", {}).get("priority_mode", "Drops Only")))
        self.mode_combo = ctk.CTkComboBox(self.main_frame, values=modes, variable=self.mode_var, width=250)
        self.mode_combo.pack(pady=5)

        # --- Appearance ---
        self.create_section(self.loc.get("appearance"))
        self.theme_var = ctk.StringVar(value=self.config.get("settings", {}).get("theme", "System"))
        ctk.CTkLabel(self.main_frame, text=self.loc.get("theme")).pack(pady=(10,0))
        self.theme_combo = ctk.CTkComboBox(self.main_frame, values=["System", "Dark", "Light"], variable=self.theme_var, command=self.change_theme)
        self.theme_combo.pack(pady=5)

        # --- Notifications ---
        self.create_section(self.loc.get("notifications"))
        
        self.notify_master = self.create_switch("notifications_enabled", self.loc.get("enable_notifications"))
        self.notify_claim = self.create_switch("notify_claim", self.loc.get("notify_claim"))
        self.notify_active = self.create_switch("notify_active_drop", self.loc.get("notify_active"))
        self.notify_finish = self.create_switch("notify_campaign_finish", self.loc.get("notify_finish"))

        # --- System ---
        self.create_section(self.loc.get("system"))
        
        self.start_minimized = self.create_switch("start_minimized", self.loc.get("start_minimized"))
        self.close_to_tray = self.create_switch("close_to_tray", self.loc.get("close_to_tray"))
        self.run_startup = self.create_switch("run_startup", self.loc.get("run_startup"))
        self.auto_start = self.create_switch("auto_start_mining", self.loc.get("auto_start"))
        
        # --- Farming & Streaks ---
        self.create_section("Farming & Streaks")
        
        # Daily Streaks
        ctk.CTkLabel(self.main_frame, text="Daily Streaks", font=("Roboto", 14, "bold")).pack(pady=(10, 5))
        self.streak_auto = self.create_bonus_switch("daily_streaks", "auto_start", "Auto-Start Daily Logic")
        
        ctk.CTkLabel(self.main_frame, text="Streak Mode").pack(pady=(5,0))
        self.streak_mode_var = ctk.StringVar(value=self.config.get("bonus", {}).get("daily_streaks", {}).get("mode", "parallel"))
        self.streak_mode_combo = ctk.CTkComboBox(self.main_frame, values=["sequential", "parallel"], variable=self.streak_mode_var)
        self.streak_mode_combo.pack(pady=5)

        # Points Mining
        ctk.CTkLabel(self.main_frame, text="Points Mining", font=("Roboto", 14, "bold")).pack(pady=(15, 5))
        self.points_auto = self.create_bonus_switch("points_farming", "auto_start", "Auto-Start Points Logic")
        
        ctk.CTkLabel(self.main_frame, text="Max Concurrent Miners").pack(pady=(5,0))
        self.max_pts_slider = ctk.CTkSlider(self.main_frame, from_=1, to=10, number_of_steps=9, command=self.update_max_pts_label)
        self.max_pts_slider.set(self.config.get("bonus", {}).get("points_farming", {}).get("max_concurrent", 3))
        self.max_pts_slider.pack(pady=5)
        self.max_pts_label = ctk.CTkLabel(self.main_frame, text=f"{int(self.max_pts_slider.get())}")
        self.max_pts_label.pack(pady=0)

        # Interval
        ctk.CTkLabel(self.main_frame, text=self.loc.get("check_interval")).pack(pady=(20,0))
        self.check_interval_slider = ctk.CTkSlider(self.main_frame, from_=30, to=300, number_of_steps=270, command=self.update_interval_label)
        self.check_interval_slider.set(self.config.get("settings", {}).get("check_interval", 60))
        self.check_interval_slider.pack(pady=10)
        self.check_interval_label = ctk.CTkLabel(self.main_frame, text=f"{int(self.check_interval_slider.get())}s")
        self.check_interval_label.pack(pady=0)


        # Save Button
        self.save_btn = ctk.CTkButton(self.main_frame, text=self.loc.get("save_settings"), command=self.save_settings, height=40, font=ctk.CTkFont(size=16, weight="bold"))
        self.save_btn.pack(pady=30)
        self.status_label = ctk.CTkLabel(self.main_frame, text="", text_color="green")
        self.status_label.pack(pady=5)

    def create_section(self, title):
         ctk.CTkLabel(self.main_frame, text=title, font=ctk.CTkFont(size=18, weight="bold")).pack(pady=(20, 10))

    def create_switch(self, key, text):
        switch = ctk.CTkSwitch(self.main_frame, text=text)
        switch.pack(pady=5, padx=20, anchor="center")
        val = self.config.get("settings", {}).get(key, True)
        # Default False for some system settings if missing
        if key in ["start_minimized", "close_to_tray", "run_startup"]:
             val = self.config.get("settings", {}).get(key, False)
             
        if val: switch.select()
        else: switch.deselect()
        if val: switch.select()
        else: switch.deselect()
        return switch

    def create_bonus_switch(self, parent_key, key, text):
        switch = ctk.CTkSwitch(self.main_frame, text=text)
        switch.pack(pady=5, padx=20, anchor="center")
        val = self.config.get("bonus", {}).get(parent_key, {}).get(key, False)
        if val: switch.select()
        else: switch.deselect()
        return switch

    def get_mode_label(self, mode_key):
        key_map = {
            "Drops Only": "mode_drops_only",
            "Smart": "mode_smart",
            "Parallel": "mode_parallel"
        }
        return self.loc.get(key_map.get(mode_key, "mode_drops_only"))
        
    def get_mode_key(self, label):
        # Reverse lookup (simple loop for now)
        if label == self.loc.get("mode_drops_only"): return "Drops Only"
        if label == self.loc.get("mode_smart"): return "Smart"
        if label == self.loc.get("mode_parallel"): return "Parallel"
        return "Drops Only"

    def update_interval_label(self, value):
        self.check_interval_label.configure(text=f"{int(value)}s")

    def update_max_pts_label(self, value):
        self.max_pts_label.configure(text=f"{int(value)}")

    def change_theme(self, choice):
        ctk.set_appearance_mode(choice)

    def change_language(self, choice):
        # Requires restart usually, but we can save it.
        pass

    def save_settings(self):
        new_settings = self.config.get("settings", {})
        new_settings["check_interval"] = int(self.check_interval_slider.get())
        
        new_settings["language"] = self.lang_var.get()
        new_settings["priority_mode"] = self.get_mode_key(self.mode_var.get())
        new_settings["theme"] = self.theme_var.get()
        
        # Notifications
        new_settings["notifications_enabled"] = bool(self.notify_master.get())
        new_settings["notify_claim"] = bool(self.notify_claim.get())
        new_settings["notify_active_drop"] = bool(self.notify_active.get())
        new_settings["notify_campaign_finish"] = bool(self.notify_finish.get())
        
        # System
        new_settings["start_minimized"] = bool(self.start_minimized.get())
        new_settings["close_to_tray"] = bool(self.close_to_tray.get())
        
        # Startup Logic
        startup_enabled = bool(self.run_startup.get())
        new_settings["run_startup"] = startup_enabled
        new_settings["auto_start_mining"] = bool(self.auto_start.get())
        if self.app:
            self.app.sys_manager.set_startup(startup_enabled)

        # Bonus Settings Save
        bonus = self.config.get("bonus", {})
        if "daily_streaks" not in bonus: bonus["daily_streaks"] = {}
        if "points_farming" not in bonus: bonus["points_farming"] = {}
        
        bonus["daily_streaks"]["auto_start"] = bool(self.streak_auto.get())
        bonus["daily_streaks"]["mode"] = self.streak_mode_var.get()
        
        bonus["points_farming"]["auto_start"] = bool(self.points_auto.get())
        bonus["points_farming"]["max_concurrent"] = int(self.max_pts_slider.get())
        
        self.config["bonus"] = bonus

        self.config["settings"] = new_settings
        self.config_manager.save_config(self.config)
        
        # Force refresh localization?
        self.loc.set_language(new_settings["language"])
        self.title_label.configure(text=self.loc.get("settings_title"))
        # Re-rendering whole UI is hard without destroy, but title change shows response

        self.status_label.configure(text=self.loc.get("saved_success"))
        self.after(3000, lambda: self.status_label.configure(text=""))

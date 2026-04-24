import customtkinter as ctk
import threading
from ui.dashboard import DashboardFrame
from ui.drops import DropsFrame
from ui.bonus import BonusFrame
from ui.settings import SettingsFrame 
from core.auth import DeviceAuthenticator
from core.miner import DropMiner
from utils.config import ConfigManager
from utils.system import SystemManager
from utils.localization import LocalizationManager

class App(ctk.CTk):
    def __init__(self, config: ConfigManager, auth: DeviceAuthenticator, miner: DropMiner):
        super().__init__()

        self.config = config
        self.auth = auth
        self.miner = miner
        self.loc = LocalizationManager(config)
        self.sys_manager = SystemManager(self, config, self.loc)

        self.title("Twitch Miner")
        self.geometry("1400x900")

        # Theme
        ctk.set_appearance_mode(self.config.get("settings", {}).get("theme", "System"))
        
        # Grid Layout
        self.grid_columnconfigure(1, weight=1)
        self.grid_rowconfigure(0, weight=1)

        self._create_sidebar()
        self._create_main_area()
        
        # System Tray & Close Logic
        self.sys_manager.setup_tray()
        if self.config.get("settings", {}).get("close_to_tray", False):
            self.protocol('WM_DELETE_WINDOW', self.hide_window)
        else:
            self.protocol('WM_DELETE_WINDOW', self.quit_app)

        # Start Tray
        self.sys_manager.run_tray()

        # Check Auth Status on Start
        self.check_auth()
        
        # Start Minimized Check
        if self.config.get("settings", {}).get("start_minimized", False):
            self.withdraw()

    def _create_sidebar(self):
        self.sidebar_frame = ctk.CTkFrame(self, width=200, corner_radius=0)
        self.sidebar_frame.grid(row=0, column=0, sticky="nsew")
        self.sidebar_frame.grid_rowconfigure(4, weight=1)

        self.logo_label = ctk.CTkLabel(self.sidebar_frame, text="Twitch Miner", font=ctk.CTkFont(size=20, weight="bold"))
        self.logo_label.grid(row=0, column=0, padx=20, pady=(20, 10))

        self.btn_dashboard = ctk.CTkButton(self.sidebar_frame, text="Dashboard", command=lambda: self.show_frame("dashboard"))
        self.btn_dashboard.grid(row=1, column=0, padx=20, pady=10)

        self.btn_drops = ctk.CTkButton(self.sidebar_frame, text="Drops & Priority", command=lambda: self.show_frame("drops"))
        self.btn_drops.grid(row=2, column=0, padx=20, pady=10)

        self.btn_streak = ctk.CTkButton(self.sidebar_frame, text="Farming & Streaks", command=lambda: self.show_frame("bonus"))
        self.btn_streak.grid(row=3, column=0, padx=20, pady=10)
        
        self.btn_settings = ctk.CTkButton(self.sidebar_frame, text="Settings", command=lambda: self.show_frame("settings"))
        self.btn_settings.grid(row=4, column=0, padx=20, pady=10)
        
        # Auth / Status Section at bottom
        self.status_label = ctk.CTkLabel(self.sidebar_frame, text="Not Connected", text_color="red")
        self.status_label.grid(row=5, column=0, padx=20, pady=10)
        
        self.btn_login = ctk.CTkButton(self.sidebar_frame, text="Login (TV Code)", command=self.start_login_flow)
        self.btn_login.grid(row=6, column=0, padx=20, pady=20)

    def _create_main_area(self):
        self.frames = {}
        
        # Initialize Frames
        self.frames["dashboard"] = DashboardFrame(self, self.miner)
        self.frames["drops"] = DropsFrame(self, self.config, self.miner.gql)
        self.frames["bonus"] = BonusFrame(self, self.config, self.miner.gql, self.miner)
        # Pass self (App) to settings for SystemManager callbacks
        self.frames["settings"] = SettingsFrame(self, self.config, app_instance=self) 

        self.show_frame("dashboard")

    def show_frame(self, name):
        frame = self.frames.get(name)
        if frame:
            frame.grid(row=0, column=1, sticky="nsew", padx=20, pady=20)
            frame.tkraise()

    def check_auth(self):
        token = self.config.get_auth_token()
        if token:
            self.status_label.configure(text="Connected", text_color="green")
            self.btn_login.configure(state="disabled", text="Logged In")
        else:
            self.status_label.configure(text="Not Connected", text_color="red")
            self.btn_login.configure(state="normal")

    def start_login_flow(self):
        # Run in thread to not block UI
        threading.Thread(target=self._login_process, daemon=True).start()

    def _login_process(self):
        try:
            self.btn_login.configure(state="disabled", text="Requesting Code...")
            data = self.auth.start_auth()
            if data:
                user_code = data.get('user_code')
                verification_uri = data.get('verification_uri')
                
                # Update UI to show code (or just rely on browser opening)
                import webbrowser
                webbrowser.open(verification_uri)
                
                # Poll
                self.after(0, lambda: self.status_label.configure(text=f"Polling... (Code: {user_code})", text_color="orange"))
                success, message = self.auth.poll_for_token(data['device_code'], data['interval'])
                if success:
                    self.after(0, lambda: self.status_label.configure(text="Connected", text_color="green"))
                    self.after(0, lambda: self.btn_login.configure(text="Logged In"))
                    # Trigger miner reload or something if needed
                    # self.miner.reload_auth()
                else:
                    self.after(0, lambda: self.status_label.configure(text="Login Failed", text_color="red"))
                    self.after(0, lambda: self.btn_login.configure(state="normal", text="Login (TV Code)"))

        except Exception as e:
            print(f"Login Error: {e}")
            self.after(0, lambda: self.status_label.configure(text="Error", text_color="red"))
            self.after(0, lambda: self.btn_login.configure(state="normal", text="Retry Login"))

    def hide_window(self):
        self.withdraw()
        # notifications handled by pystray ideally, but we can do a toast
        from utils.notifications import NotificationManager
        NotificationManager(self.config).send_notification("Twitch Miner", "Application minimized to tray.", event_type="generic")
        
    def show_window(self):
        self.deiconify()
        
    def quit_app(self):
        # Cleanup
        if self.miner: self.miner.stop()
        if self.sys_manager: self.sys_manager.stop_tray()
        self.quit()
        self.destroy()

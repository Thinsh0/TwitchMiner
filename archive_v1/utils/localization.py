class LocalizationManager:
    def __init__(self, config_manager):
        self.config = config_manager
        self.current_lang = self.config.get("settings", {}).get("language", "EN")
        self.translations = {
            "EN": {
                "settings_title": "Settings",
                "check_interval": "Check Interval",
                "notifications": "Notifications",
                "enable_notifications": "Enable Notifications (Master)",
                "notify_claim": "Notify on Drop Claim",
                "notify_active": "Notify on New Active Drop",
                "notify_finish": "Notify on Campaign Finish",
                "save_settings": "Save Settings",
                "saved_success": "Settings Saved Successfully!",
                "appearance": "Appearance",
                "theme": "Theme",
                "general": "General",
                "language": "Language",
                "concurrency": "Concurrency Mode",
                "system": "System",
                "start_minimized": "Start Minimized",
                "close_to_tray": "Close to Tray",
                "run_startup": "Run on Windows Startup",
                "mode_drops_only": "Drops Only (Safe)",
                "mode_smart": "Smart (Pause Points)",
                "mode_parallel": "Parallel (Exp.)",
                "auto_start": "Auto-Start Mining"
            },
            "FR": {
                "settings_title": "Paramètres",
                "check_interval": "Intervalle de vérification",
                "notifications": "Notifications",
                "enable_notifications": "Activer les notifications (Master)",
                "notify_claim": "Notifier lors du Claim",
                "notify_active": "Notifier nouveau Drop actif",
                "notify_finish": "Notifier fin de campagne",
                "save_settings": "Sauvegarder",
                "saved_success": "Paramètres sauvegardés !",
                "appearance": "Apparence",
                "theme": "Thème",
                "general": "Général",
                "language": "Langue",
                "concurrency": "Mode de Concurrence",
                "system": "Système",
                "start_minimized": "Démarrer réduit",
                "close_to_tray": "Fermer dans le Tray",
                "run_startup": "Lancer au démarrage Windows",
                "mode_drops_only": "Drops Uniquement (Sécure)",
                "mode_smart": "Intelligent (Pause Points)",
                "mode_parallel": "Parallèle (Exp.)",
                "auto_start": "Démarrage Automatique du Minage"
            }
        }

    def get(self, key):
        lang_dict = self.translations.get(self.current_lang, self.translations["EN"])
        return lang_dict.get(key, key)

    def set_language(self, lang_code):
        if lang_code in self.translations:
            self.current_lang = lang_code

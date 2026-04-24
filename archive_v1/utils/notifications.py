import logging
try:
    from plyer import notification
except ImportError:
    notification = None

class NotificationManager:
    def __init__(self, config_manager):
        self.config_manager = config_manager
        
    def _is_enabled(self, setting_key):
        """Check if a specific notification setting is enabled in config."""
        settings = self.config_manager.config.get("settings", {})
        # Global Master Switch - DEFAULT ON
        if not settings.get("notifications_enabled", True):
            return False
            
        # Specific Setting - DEFAULT ON
        return settings.get(f"notify_{setting_key}", True)

    def send_notification(self, title, message, event_type="generic"):
        """
        Send a desktop notification if enabled.
        event_type options: 'active_drop', 'claim', 'campaign_finish'
        """
        if not notification:
            print("[Notification] Plyer not installed. Skipping toast.")
            return

        if self._is_enabled(event_type):
            try:
                notification.notify(
                    title=title,
                    message=message,
                    app_name="Twitch DropMiner",
                    app_icon=None, # Icon path could be added here
                    timeout=5,
                )
                print(f"[Notification] Sent: {title} - {message}")
            except Exception as e:
                print(f"[Notification] Error sending toast: {e}")

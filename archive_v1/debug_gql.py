from utils.config import ConfigManager
from core.api import TwitchGQL
import json

# Initialize
cfg = ConfigManager()
gql = TwitchGQL(cfg)

target_channel = "poko" # From screenshot

print(f"--- Debugging Points for {target_channel} ---")
# 1. Test Points Query
try:
    data = gql.get_channel_points_context(target_channel)
    print(json.dumps(data, indent=2))
except Exception as e:
    print(f"Points Query Error: {e}")

print(f"\n--- Debugging Streak for {target_channel} ---")
# 2. Test Streak Query (Probing 2)
query = """
query DebugStreak($login: String!) {
    channel(name: $login) {
        id
        viewer {
           watchStreak {
               score
               currentStreak
           }
        }
    }
}
"""
try:
    res = gql.post("DebugStreak", query, {"login": target_channel})
    print(json.dumps(res, indent=2))
except Exception as e:
     print(f"Streak Query Error: {e}")

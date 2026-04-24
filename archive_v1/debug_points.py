from utils.config import ConfigManager
from core.api import TwitchGQL
import json

def debug_points():
    cfg = ConfigManager()
    gql = TwitchGQL(cfg)
    
    target = "poko"
    print(f"Testing Points GQL for: {target}")
    
    # 1. Test get_channel_points_context
    try:
        data = gql.get_channel_points_context(target)
        print("\n--- Raw Response ---")
        print(json.dumps(data, indent=2))
        
        # Simulate parsing
        if data:
            ch_data = data.get("data", {}).get("channel", {})
            if ch_data and ch_data.get("self"):
                pts = ch_data["self"].get("communityPoints", {}).get("balance", 0)
                print(f"\nParsed Points: {pts}")
            else:
                print("\nParsed: Limit/Err (Missing 'self' or 'channel')")
        else:
            print("\nResponse was None")
            
    except Exception as e:
        print(f"\nException: {e}")

if __name__ == "__main__":
    debug_points()

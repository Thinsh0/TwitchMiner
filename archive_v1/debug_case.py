from utils.config import ConfigManager
from core.api import TwitchGQL
import json

def debug_points_case():
    cfg = ConfigManager()
    gql = TwitchGQL(cfg)
    
    # Test CAPITALIZED
    target = "Poko" 
    print(f"Testing Points GQL for: {target} (Capitalized)")
    
    try:
        data = gql.get_channel_points_context(target)
        print("\n--- Raw Response ---")
        print(json.dumps(data, indent=2))
        
        if data:
            ch_data = data.get("data", {}).get("channel", {})
            if ch_data:
                print("Channel Data Found")
            else:
                 print("Channel Data is None! (Case Sensitivity Confirmed)")
        
    except Exception as e:
        print(f"\nException: {e}")

if __name__ == "__main__":
    debug_points_case()

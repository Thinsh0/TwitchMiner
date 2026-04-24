from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
import threading
import uvicorn
import asyncio
from pydantic import BaseModel

# Import existing core
from utils.config import ConfigManager
from core.auth import DeviceAuthenticator
from core.miner import DropMiner
from core.bonus import BonusManager # Assuming we might want to expose bonus stats too

app = FastAPI()

# Enable CORS for local development
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Global State
class ServiceState:
    config: ConfigManager = None
    auth: DeviceAuthenticator = None
    miner: DropMiner = None
    bonus: BonusManager = None

state = ServiceState()

# --- Models ---
class PriorityUpdate(BaseModel):
    games: list[str]
    streamers: list[str]

# --- Routes ---

@app.get("/status")
def get_status():
    if not state.miner:
        return {"status": "Offline"}
    
    # Miner status
    s = state.miner.get_status()
    
    # Auth status
    token = state.config.get_auth_token()
    s["authenticated"] = bool(token)
    
    return s

@app.post("/start")
def start_miner():
    if state.miner:
        state.miner.start()
        return {"message": "Started"}
    return {"error": "Miner not initialized"}

@app.post("/stop")
def stop_miner():
    if state.miner:
        state.miner.stop()
        return {"message": "Stopped"}
    return {"error": "Miner not initialized"}

@app.get("/config")
def get_config():
    return state.config.config

@app.post("/priorities")
def update_priorities(p: PriorityUpdate):
    state.config.set("priorities", {"games": p.games, "streamers": p.streamers})
    return {"message": "Updated"}

@app.get("/inventory")
def get_inventory():
    # Helper to proxy GQL inventory call
    if state.miner and state.miner.gql:
        return state.miner.gql.get_drops_campaigns()
    return {}

@app.get("/search")
def search_games(query: str):
    if state.miner and state.miner.gql:
        return state.miner.gql.search_games(query)
    return {}

# --- Auth Routes ---
@app.post("/auth/start")
def auth_start():
    if not state.auth:
        return {"error": "Auth service not available"}
    
    data = state.auth.start_auth()
    if data:
        # We also need to help user open browser or return the URL
        # For remote/headless, returning URL is best.
        return data
    return {"error": "Failed to start auth flow"}

@app.post("/auth/poll")
def auth_poll(data: dict):
    # Expects {'device_code': '', 'interval': 5}
    device_code = data.get("device_code")
    interval = data.get("interval", 5)
    
    if not state.auth or not device_code:
        return {"success": False, "message": "Invalid request"}

    # This is blocking, but FastAPI handles async if we use async def or background threads.
    # state.auth.poll_for_token is BLOCKING loop. We should probably NOT do this in the main thread request directly 
    # OR we let the frontend poll us while we check ONCE?
    # Actually DeviceAuthenticator.poll_for_token LOOPS until done. That's bad for HTTP request. 
    # We should change it to "check_token" (one shot) or run the loop in a thread and status check.
    
    # Simplest for now: User calls "poll" and we run the LOOP in a background thread, 
    # returning "started" immediatley. Then user checks /status to see if 'authenticated' becomes true.
    
    def _run_poll():
        success, msg = state.auth.poll_for_token(device_code, interval)
        if success:
             print("[API Auth] Authenticated!")
             # Trigger miner reload if needed?
        else:
             print(f"[API Auth] Failed: {msg}")

    threading.Thread(target=_run_poll, daemon=True).start()
    return {"success": True, "message": "Polling started in background", "blocking": False}
    
@app.post("/auth/poll_blocking")
def auth_poll_blocking(data: dict):
    # Useful if frontend wants to wait on the request (timeout risk)
    device_code = data.get("device_code")
    interval = data.get("interval", 5)
    
    if not state.auth: return {"success": False}
    
    success, msg = state.auth.poll_for_token(device_code, interval)
    return {"success": success, "message": msg}

# --- Server Lifecycle ---

@app.get("/bonus/status")
def get_bonus_status():
    if not state.bonus:
        return {"error": "Bonus service not initialized"}
    
    return {
        "daily_streak": {
            "status": state.bonus.daily.status,
            "is_running": state.bonus.daily.is_running,
            "current": state.bonus.daily.current_channel
        },
        "points_farming": {
            "is_running": state.bonus.points.is_running,
            "channels": state.bonus.points.stats # dict of channel -> {points, status}
        }
    }

@app.post("/settings/update")
def update_settings(data: dict):
    # safe update for arbitrary keys? 
    # data: { "section": "settings", "key": "theme", "value": "Dark" }
    section = data.get("section")
    key = data.get("key")
    value = data.get("value")
    
    if section and key:
        current = state.config.get(section, {})
        current[key] = value
        state.config.set(section, current)
        return {"success": True}
    return {"error": "Invalid params"}

# --- Server Lifecycle ---

def start_api_server(config, auth, miner, bonus=None, port=36969):
    state.config = config
    state.auth = auth
    state.miner = miner
    state.bonus = bonus
    
    threading.Thread(target=lambda: uvicorn.run(app, host="127.0.0.1", port=port, log_level="info"), daemon=True).start()

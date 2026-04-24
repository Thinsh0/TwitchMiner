use crate::models::MinerStatus;
use crate::api::TwitchGQL;
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use serde_json::Value;

pub struct MinerState {
    pub status: MinerStatus,
    pub is_running: bool,
    pub task: Option<JoinHandle<()>>,
}

impl MinerState {
    pub fn new() -> Self {
        Self {
            status: MinerStatus {
                status: "Idle".to_string(),
                ..Default::default()
            },
            is_running: false,
            task: None,
        }
    }

    pub fn get_status(&self) -> MinerStatus {
        self.status.clone()
    }

    pub fn update_and_notify(&mut self, app: &AppHandle, new_status: MinerStatus) {
        self.status = new_status.clone();
        
        // Send real-time update to frontend
        let _ = app.emit("miner-status-update", new_status);
    }
}

async fn check_and_claim_drops(gql: &TwitchGQL, store: &Arc<tauri_plugin_store::Store<tauri::Wry>>, current_user_id: &str, inventory: &Value) {
    // 1. Check Completed Drops List
    if let Some(drops) = inventory.pointer("/data/currentUser/inventory/drops").and_then(|v| v.as_array()) {
        for drop in drops {
            let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
            let pct = drop.get("percentAwarded").and_then(|v| v.as_i64()).unwrap_or(0);
            
            if !is_claimed && pct >= 100 {
                let mut claim_id = drop.pointer("/self/dropInstanceID").and_then(|v| v.as_str()).map(|s| s.to_string());
                
                if claim_id.is_none() {
                    let camp_id = drop.pointer("/campaign/id").and_then(|v| v.as_str());
                    let drop_def_id = drop.get("id").and_then(|v| v.as_str());
                    if let (Some(c), Some(d)) = (camp_id, drop_def_id) {
                        claim_id = Some(format!("{}#{}#{}", current_user_id, c, d));
                    }
                }

                if let Some(c_id) = claim_id {
                    let _ = gql.claim_drop(store, &c_id).await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        }
    }

    // 2. Check Active Campaigns Progress
    if let Some(campaigns) = inventory.pointer("/data/currentUser/inventory/dropCampaignsInProgress").and_then(|v| v.as_array()) {
        for camp in campaigns {
            if let Some(time_drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                for drop in time_drops {
                    let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
                    let cur_mins = drop.pointer("/self/currentMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                    let req_mins = drop.get("requiredMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                    
                    if !is_claimed && req_mins > 0 && cur_mins >= req_mins {
                        let mut claim_id = drop.pointer("/self/dropInstanceID").and_then(|v| v.as_str()).map(|s| s.to_string());
                        
                        if claim_id.is_none() {
                            let camp_id = camp.get("id").and_then(|v| v.as_str());
                            let drop_def_id = drop.get("id").and_then(|v| v.as_str());
                            if let (Some(c), Some(d)) = (camp_id, drop_def_id) {
                                claim_id = Some(format!("{}#{}#{}", current_user_id, c, d));
                            }
                        }

                        if let Some(c_id) = claim_id {
                            let _ = gql.claim_drop(store, &c_id).await;
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        }
                    }
                }
            }
        }
    }
}

fn extract_image_url(drop: &Value) -> Option<String> {
    if let Some(edges) = drop.get("benefitEdges").and_then(|v| v.as_array()) {
        if let Some(first) = edges.first() {
            if let Some(url) = first.pointer("/benefit/imageAssetURL").and_then(|v| v.as_str()) {
                return Some(url.to_string());
            }
        }
    }
    drop.get("imageURL").and_then(|v| v.as_str()).map(|s| s.to_string())
}

#[tauri::command]
pub async fn start_miner(app: AppHandle, state: tauri::State<'_, Arc<Mutex<MinerState>>>) -> Result<(), String> {
    let mut s = state.lock().await;
    if s.is_running {
        return Ok(());
    }

    s.is_running = true;
    let mut new_status = s.status.clone();
    new_status.status = "Starting...".to_string();
    new_status.is_running = true;
    s.update_and_notify(&app, new_status);

    let state_clone = state.inner().clone();
    let app_clone = app.clone();
    
    let store = match app.store("config.json") {
        Ok(st) => st,
        Err(e) => return Err(e.to_string()),
    };

    let handle = tokio::spawn(async move {
        let gql = TwitchGQL::new();
        
        loop {
            // Check if still running
            {
                let s_guard = state_clone.lock().await;
                if !s_guard.is_running {
                    break;
                }
            }

            {
                let mut s_guard = state_clone.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.status = "Checking Inventory...".to_string();
                s_guard.update_and_notify(&app_clone, new_status);
            }

            // 1. Get User Info
            let ui_res = gql.get_user_info(&store).await;
            let current_user_id = if let Ok(ui) = &ui_res {
                ui.pointer("/data/currentUser/id").and_then(|v| v.as_str()).map(|s| s.to_string())
            } else {
                None
            };

            if let Some(uid) = current_user_id {
                if let Ok(inventory) = gql.get_inventory(&store).await {
                    // Check and Claim
                    check_and_claim_drops(&gql, &store, &uid, &inventory).await;

                    // Find Target Game
                    let mut target_game_id = None;
                    let mut target_game_name = None;
                    
                    if let Some(campaigns) = inventory.pointer("/data/currentUser/inventory/dropCampaignsInProgress").and_then(|v| v.as_array()) {
                        for camp in campaigns {
                            if let Some(drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                                // Find an incomplete drop
                                let mut has_incomplete = false;
                                for drop in drops {
                                    let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
                                    let cur_mins = drop.pointer("/self/currentMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let req_mins = drop.get("requiredMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                                    if !is_claimed && cur_mins < req_mins {
                                        has_incomplete = true;
                                        break;
                                    }
                                }
                                
                                if has_incomplete {
                                    target_game_id = camp.pointer("/game/id").and_then(|v| v.as_str()).map(|s| s.to_string());
                                    target_game_name = camp.pointer("/game/displayName").and_then(|v| v.as_str()).map(|s| s.to_string());
                                    break;
                                }
                            }
                        }
                    }

                    if let (Some(game_id), Some(game_name)) = (target_game_id, target_game_name) {
                        {
                            let mut s_guard = state_clone.lock().await;
                            let mut new_status = s_guard.status.clone();
                            new_status.status = format!("Looking for {} streamer...", game_name);
                            s_guard.update_and_notify(&app_clone, new_status);
                        }

                        if let Ok(streams) = gql.get_live_streams(&store, &game_id).await {
                            let mut target_streamer_login = None;
                            let mut target_streamer_name = None;
                            let mut target_streamer_image = None;
                            let mut target_stream_id = None;
                            let mut target_channel_id = None;

                            if let Some(edges) = streams.pointer("/data/game/streams/edges").and_then(|v| v.as_array()) {
                                for edge in edges {
                                    let tags = edge.pointer("/node/tags").and_then(|v| v.as_array());
                                    let has_drops = tags.map(|t| t.iter().any(|tag| tag.get("id").and_then(|id| id.as_str()) == Some("c2542d6d-cd10-4532-919b-3d19f30a768b"))).unwrap_or(false);
                                    
                                    if has_drops {
                                        target_streamer_login = edge.pointer("/node/broadcaster/login").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        target_streamer_name = edge.pointer("/node/broadcaster/displayName").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        target_streamer_image = edge.pointer("/node/broadcaster/profileImageURL").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        target_stream_id = edge.pointer("/node/id").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        target_channel_id = edge.pointer("/node/broadcaster/id").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        break;
                                    }
                                }
                            }

                            if let Some(streamer_login) = target_streamer_login {
                                {
                                    let mut s_guard = state_clone.lock().await;
                                    let mut new_status = s_guard.status.clone();
                                    new_status.current_streamer = target_streamer_name.clone();
                                    new_status.streamer_image_url = target_streamer_image.clone();
                                    new_status.status = format!("Watching {}...", target_streamer_name.unwrap_or_default());
                                    s_guard.update_and_notify(&app_clone, new_status);
                                }

                                let _ = gql.get_playback_access_token(&store, &streamer_login).await;
                                
                                if let Ok(spade_url) = gql.get_spade_url(&streamer_login).await {
                                    let mut loop_count = 15; // default watch time before re-check
                                    
                                    while loop_count > 0 {
                                        {
                                            let s_guard = state_clone.lock().await;
                                            if !s_guard.is_running {
                                                break;
                                            }
                                        }

                                        // Update UI Progress from Inventory
                                        if let Ok(inv) = gql.get_inventory(&store).await {
                                            if let Some(campaigns) = inv.pointer("/data/currentUser/inventory/dropCampaignsInProgress").and_then(|v| v.as_array()) {
                                                let mut found = false;
                                                for camp in campaigns {
                                                    let camp_game_id = camp.pointer("/game/id").and_then(|v| v.as_str());
                                                    if camp_game_id == Some(&game_id) {
                                                        if let Some(drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                                                            for drop in drops {
                                                                let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
                                                                let cur_mins = drop.pointer("/self/currentMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                                                                let req_mins = drop.get("requiredMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                                                                
                                                                if !is_claimed && cur_mins < req_mins {
                                                                    let mut s_guard = state_clone.lock().await;
                                                                    let mut new_status = s_guard.status.clone();
                                                                    new_status.current_drop = drop.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                                                                    new_status.drop_image_url = extract_image_url(drop);
                                                                    new_status.progress = if req_mins > 0 { (cur_mins as f64 / req_mins as f64) * 100.0 } else { 0.0 };
                                                                    new_status.status = format!("Mining: {}m / {}m", cur_mins, req_mins);
                                                                    s_guard.update_and_notify(&app_clone, new_status);
                                                                    loop_count = (req_mins - cur_mins).max(1);
                                                                    found = true;
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                    }
                                                    if found { break; }
                                                }
                                            }
                                        }

                                        // Spade Ping
                                        let payload = serde_json::json!({
                                            "event": "minute-watched",
                                            "properties": {
                                                "broadcast_id": target_stream_id,
                                                "channel_id": target_channel_id,
                                                "channel": streamer_login,
                                                "hidden": false,
                                                "live": true,
                                                "location": "channel",
                                                "logged_in": true,
                                                "muted": false,
                                                "player": "site",
                                                "user_id": uid
                                            }
                                        });
                                        let _ = gql.send_spade_minute_watched(&spade_url, payload).await;

                                        // Claim points
                                        if let Ok(pts_res) = gql.get_channel_points_context(&store, &streamer_login).await {
                                            if let Some(balance) = pts_res.pointer("/data/channel/self/communityPoints/balance").and_then(|v| v.as_i64()) {
                                                let mut s_guard = state_clone.lock().await;
                                                let mut new_status = s_guard.status.clone();
                                                if balance >= 1000 {
                                                    new_status.current_points = format!("{:.1}K", balance as f64 / 1000.0);
                                                } else {
                                                    new_status.current_points = balance.to_string();
                                                }
                                                s_guard.update_and_notify(&app_clone, new_status);
                                            }
                                            
                                            if let Some(claim_id) = pts_res.pointer("/data/channel/self/communityPoints/availableClaim/id").and_then(|v| v.as_str()) {
                                                if let Some(ch_id) = pts_res.pointer("/data/channel/id").and_then(|v| v.as_str()) {
                                                    let _ = gql.claim_community_points(&store, ch_id, claim_id).await;
                                                }
                                            }
                                        }

                                        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                                        loop_count -= 1;
                                    }
                                }
                            } else {
                                {
                                    let mut s_guard = state_clone.lock().await;
                                    let mut new_status = s_guard.status.clone();
                                    new_status.status = format!("No drops-enabled streams found for {}", game_name);
                                    new_status.current_streamer = None;
                                    s_guard.update_and_notify(&app_clone, new_status);
                                }
                                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                            }
                        }
                    } else {
                        {
                            let mut s_guard = state_clone.lock().await;
                            let mut new_status = s_guard.status.clone();
                            new_status.status = "No active drops found.".to_string();
                            new_status.current_streamer = None;
                            new_status.current_drop = None;
                            new_status.progress = 0.0;
                            s_guard.update_and_notify(&app_clone, new_status);
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                    }
                }
            } else {
                {
                    let mut s_guard = state_clone.lock().await;
                    let mut new_status = s_guard.status.clone();
                    new_status.status = "Authentication Error".to_string();
                    s_guard.update_and_notify(&app_clone, new_status);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            }
        }

        let mut s_guard = state_clone.lock().await;
        let mut new_status = s_guard.status.clone();
        new_status.status = "Idle".to_string();
        new_status.is_running = false;
        s_guard.update_and_notify(&app_clone, new_status);
    });

    s.task = Some(handle);
    Ok(())
}

#[tauri::command]
pub async fn stop_miner(app: AppHandle, state: tauri::State<'_, Arc<Mutex<MinerState>>>) -> Result<(), String> {
    let mut s = state.lock().await;
    s.is_running = false;
    let mut new_status = s.status.clone();
    new_status.is_running = false;
    new_status.status = "Stopped".to_string();
    s.update_and_notify(&app, new_status);
    
    if let Some(task) = s.task.take() {
        task.abort();
    }
    Ok(())
}
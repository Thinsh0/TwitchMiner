use crate::models::{MinerStatus, ActiveDropInfo, DropCampaign, DropItem};
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
                let mut claim_id = json_id_to_string(drop.pointer("/self/dropInstanceID"));
                
                if claim_id.is_none() {
                    let camp_id = json_id_to_string(drop.pointer("/campaign/id"));
                    let drop_def_id = json_id_to_string(drop.get("id"));
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
                        let mut claim_id = json_id_to_string(drop.pointer("/self/dropInstanceID"));
                        
                        if claim_id.is_none() {
                            let camp_id = json_id_to_string(camp.get("id"));
                            let drop_def_id = json_id_to_string(drop.get("id"));
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

fn json_id_to_string(value: Option<&Value>) -> Option<String> {
    value.and_then(|v| {
        v.as_str().map(|s| s.to_string())
            .or_else(|| v.as_u64().map(|n| n.to_string()))
    })
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

fn parse_drop_item(drop: &Value) -> Option<DropItem> {
    let id = json_id_to_string(drop.get("id"))?;
    let name = drop.get("name").and_then(|v| v.as_str()).unwrap_or("Drop Reward").to_string();
    let required_minutes = drop.get("requiredMinutesWatched").and_then(|v| v.as_u64()).unwrap_or(0);
    let current_minutes = drop.pointer("/self/currentMinutesWatched").and_then(|v| v.as_u64()).unwrap_or(0);
    let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
    Some(DropItem {
        id,
        name,
        required_minutes,
        current_minutes,
        is_claimed,
        image_url: extract_image_url(drop),
    })
}

fn parse_campaign_node(camp: &Value, status: &str) -> Option<DropCampaign> {
    let id = json_id_to_string(camp.get("id"))?;
    let name = camp.get("name").and_then(|v| v.as_str()).unwrap_or("Drop Campaign").to_string();
    let game_name = camp.pointer("/game/displayName")
        .and_then(|v| v.as_str())
        .or_else(|| camp.pointer("/game/name").and_then(|v| v.as_str()))
        .unwrap_or("Unknown Game")
        .to_string();
    let image_url = camp.pointer("/game/boxArtURL").and_then(|v| v.as_str()).map(|s| s.to_string());
    let start_date = camp.get("startAt").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let end_date = camp.get("endAt").and_then(|v| v.as_str()).unwrap_or_default().to_string();

    let mut drops = Vec::new();
    if let Some(time_drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
        for drop in time_drops {
            if let Some(item) = parse_drop_item(drop) {
                drops.push(item);
            }
        }
    }

    Some(DropCampaign {
        id,
        name,
        game_name,
        image_url,
        status: status.to_string(),
        start_date,
        end_date,
        drops,
    })
}

fn parse_drop_campaigns(inventory: &Value) -> Vec<DropCampaign> {
    let mut parsed: Vec<DropCampaign> = Vec::new();
    let mut by_id: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    // 1. In-progress campaigns
    if let Some(campaigns) = inventory.pointer("/data/currentUser/inventory/dropCampaignsInProgress").and_then(|v| v.as_array()) {
        for camp in campaigns {
            if let Some(campaign) = parse_campaign_node(camp, "ACTIVE") {
                by_id.insert(campaign.id.clone(), parsed.len());
                parsed.push(campaign);
            }
        }
    }

    // 2. Available campaigns (not yet started)
    if let Some(campaigns) = inventory.pointer("/data/currentUser/inventory/viewerDropCampaigns").and_then(|v| v.as_array()) {
        for camp in campaigns {
            if let Some(campaign) = parse_campaign_node(camp, "AVAILABLE") {
                if let Some(idx) = by_id.get(&campaign.id) {
                    // Merge drops into existing campaign
                    for drop in campaign.drops {
                        if !parsed[*idx].drops.iter().any(|d| d.id == drop.id) {
                            parsed[*idx].drops.push(drop);
                        }
                    }
                } else {
                    by_id.insert(campaign.id.clone(), parsed.len());
                    parsed.push(campaign);
                }
            }
        }
    }

    // 3. Completed drops from inventory.drops (may have been removed from inProgress)
    if let Some(drops) = inventory.pointer("/data/currentUser/inventory/drops").and_then(|v| v.as_array()) {
        for drop in drops {
            let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
            let pct = drop.get("percentAwarded").and_then(|v| v.as_i64()).unwrap_or(0);
            if !is_claimed && pct < 100 {
                // Not completed yet, already handled by campaign parsing above
                continue;
            }

            let camp_id = json_id_to_string(drop.pointer("/campaign/id")).unwrap_or_default();
            let camp_name = drop.pointer("/campaign/name").and_then(|v| v.as_str()).unwrap_or("Completed Drops").to_string();
            let game_name = drop.pointer("/campaign/game/displayName")
                .and_then(|v| v.as_str())
                .or_else(|| drop.pointer("/campaign/game/name").and_then(|v| v.as_str()))
                .unwrap_or("Unknown Game")
                .to_string();
            let game_image = drop.pointer("/campaign/game/boxArtURL").and_then(|v| v.as_str()).map(|s| s.to_string());

            if let Some(item) = parse_drop_item(drop) {
                if let Some(idx) = by_id.get(&camp_id) {
                    if !parsed[*idx].drops.iter().any(|d| d.id == item.id) {
                        parsed[*idx].drops.push(item);
                    }
                } else {
                    let campaign = DropCampaign {
                        id: camp_id.clone(),
                        name: camp_name,
                        game_name,
                        image_url: game_image,
                        status: if is_claimed { "COMPLETED".to_string() } else { "READY".to_string() },
                        start_date: String::new(),
                        end_date: String::new(),
                        drops: vec![item],
                    };
                    by_id.insert(camp_id, parsed.len());
                    parsed.push(campaign);
                }
            }
        }
    }

    parsed
}

fn collect_incomplete_time_based_drops_for_game(
    inventory: &Value,
    game_id: &str,
) -> Vec<(String, String, Option<String>, i64, i64, i64)> {
    let mut active_drops = Vec::new();
    let mut seen_drop_ids = std::collections::HashSet::new();
    let campaign_paths = [
        "/data/currentUser/inventory/dropCampaignsInProgress",
        "/data/currentUser/inventory/viewerDropCampaigns",
    ];

    for path in campaign_paths {
        if let Some(campaigns) = inventory.pointer(path).and_then(|v| v.as_array()) {
            for camp in campaigns {
                if json_id_to_string(camp.pointer("/game/id")).as_deref() != Some(game_id) {
                    continue;
                }
                if let Some(drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                    // Per campaign: only expose the ONE active drop — the incomplete drop
                    // with the least remaining watch time (Twitch campaigns are sequential).
                    let mut best: Option<(String, String, Option<String>, i64, i64, i64)> = None;
                    for drop in drops {
                        let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
                        let cur_mins = drop.pointer("/self/currentMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                        let req_mins = drop.get("requiredMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                        let percent = drop.get("percentAwarded").and_then(|v| v.as_i64()).unwrap_or(0);
                        if !is_claimed && req_mins > 0 && cur_mins < req_mins {
                            let drop_id = json_id_to_string(drop.get("id")).unwrap_or_else(|| {
                                drop.get("name").and_then(|v| v.as_str()).unwrap_or("Drop Reward").to_string()
                            });
                            if seen_drop_ids.contains(&drop_id) {
                                continue;
                            }
                            let remaining = req_mins - cur_mins;
                            let name = drop.get("name").and_then(|v| v.as_str()).unwrap_or("Drop Reward").to_string();
                            let entry = (drop_id, name, extract_image_url(drop), cur_mins, req_mins, percent);
                            let is_better = best.as_ref().map_or(true, |(_, _, _, bc, br, _)| (*br - *bc) > remaining);
                            if is_better {
                                best = Some(entry);
                            }
                        }
                    }
                    if let Some(entry) = best {
                        seen_drop_ids.insert(entry.0.clone());
                        active_drops.push(entry);
                    }
                }
            }
        }
    }

    active_drops
}

fn summarize_active_drops(
    active_drops: &[(String, String, Option<String>, i64, i64, i64)],
) -> (Option<String>, Option<String>, f64, i64, i64, i64, Vec<ActiveDropInfo>) {
    let total_required: i64 = active_drops.iter().map(|(_, _, _, _, req, _)| *req).sum();
    let total_current: i64 = active_drops.iter().map(|(_, _, _, cur, req, percent)| {
        if *cur > 0 {
            *cur
        } else {
            ((*req as f64) * (*percent as f64 / 100.0)).round() as i64
        }
    }).sum();
    let min_remaining = active_drops.iter().map(|(_, _, _, cur, req, percent)| {
        let effective_current = if *cur > 0 {
            *cur
        } else {
            ((*req as f64) * (*percent as f64 / 100.0)).round() as i64
        };
        (req - effective_current).max(1)
    }).min().unwrap_or(15);
    // Overall progress (sum) kept for compat
    let overall_progress = if total_required > 0 {
        (total_current as f64 / total_required as f64) * 100.0
    } else {
        0.0
    };
    let current_drop = match active_drops.len() {
        0 => None,
        1 => active_drops.first().map(|(_, name, _, _, _, _)| name.clone()),
        count => Some(format!("{} drops actifs en cours", count)),
    };
    let image = active_drops.iter().find_map(|(_, _, image, _, _, _)| image.clone());
    // Build per-drop detail list with individual progress
    let drop_infos: Vec<ActiveDropInfo> = active_drops.iter().map(|(id, name, img, cur, req, percent)| {
        let effective_current = if *cur > 0 {
            *cur
        } else {
            ((*req as f64) * (*percent as f64 / 100.0)).round() as i64
        };
        let pct = if *req > 0 {
            (effective_current as f64 / *req as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        ActiveDropInfo {
            id: id.clone(),
            name: name.clone(),
            image_url: img.clone(),
            current_minutes: effective_current,
            required_minutes: *req,
            progress: pct,
        }
    }).collect();
    (current_drop, image, overall_progress, total_current, total_required, min_remaining, drop_infos)
}

fn parse_available_campaigns(response: &Value, fallback_game_name: &str) -> Vec<DropCampaign> {
    let mut parsed = Vec::new();

    if let Some(campaigns) = response.pointer("/data/channel/viewerDropCampaigns").and_then(|v| v.as_array()) {
        for camp in campaigns {
            if let Some(mut campaign) = parse_campaign_node(camp, "AVAILABLE") {
                if campaign.game_name == "Unknown Game" {
                    campaign.game_name = fallback_game_name.to_string();
                }
                parsed.push(campaign);
            }
        }
    }

    parsed
}

fn campaign_exists(campaigns: &[DropCampaign], candidate: &DropCampaign) -> bool {
    campaigns.iter().any(|c| !candidate.id.is_empty() && c.id == candidate.id)
}

#[tauri::command]
pub async fn get_drop_campaigns(app: AppHandle) -> Result<Vec<DropCampaign>, String> {
    let store = app.store("config.json").map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    let gql = TwitchGQL::new();
    let inventory = gql.get_inventory(&store).await?;
    let mut campaigns = parse_drop_campaigns(&inventory);

    let priority_games = store.get("app_config")
        .and_then(|value| serde_json::from_value::<crate::models::AppConfig>(value).ok())
        .map(|config| config.priorities.games)
        .unwrap_or_default();

    for game_name in priority_games {
        let already_has_game = campaigns.iter().any(|campaign| campaign.game_name.eq_ignore_ascii_case(&game_name));
        if already_has_game {
            continue;
        }

        println!("get_drop_campaigns: checking configured game {}", game_name);
        let search = match gql.search_games(&store, &game_name).await {
            Ok(value) => value,
            Err(e) => {
                println!("get_drop_campaigns: game search failed for {}: {}", game_name, e);
                continue;
            }
        };

        let game_id = json_id_to_string(search.pointer("/data/searchFor/games/edges/0/item/id"));
        let Some(ref game_id) = game_id else {
            println!("get_drop_campaigns: no game id found for {}", game_name);
            continue;
        };

        let streams = match gql.get_live_streams(&store, game_id).await {
            Ok(value) => value,
            Err(e) => {
                println!("get_drop_campaigns: live streams failed for {}: {}", game_name, e);
                continue;
            }
        };

        if let Some(edges) = streams.pointer("/data/game/streams/edges").and_then(|v| v.as_array()) {
            for edge in edges {
                let tags = edge.pointer("/node/tags").and_then(|v| v.as_array());
                let freeform_tags = edge.pointer("/node/freeformTags").and_then(|v| v.as_array());
                let has_drops_tag = tags.map(|t| t.iter().any(|tag| tag.get("id").and_then(|id| id.as_str()) == Some("c2542d6d-cd10-4532-919b-3d19f30a768b"))).unwrap_or(false)
                    || freeform_tags.map(|t| t.iter().any(|tag| tag.get("name").and_then(|name| name.as_str()) == Some("DropsEnabled"))).unwrap_or(false);

                if !has_drops_tag {
                    continue;
                }

                let channel_id = json_id_to_string(edge.pointer("/node/broadcaster/id"));
                let Some(ref channel_id) = channel_id else {
                    continue;
                };

                let available = match gql.get_available_drops(&store, channel_id).await {
                    Ok(value) => value,
                    Err(e) => {
                        println!("get_drop_campaigns: available drops failed for {}: {}", game_name, e);
                        continue;
                    }
                };

                let found = parse_available_campaigns(&available, &game_name);
                for campaign in found {
                    if campaign.game_name.eq_ignore_ascii_case(&game_name) && !campaign_exists(&campaigns, &campaign) {
                        campaigns.push(campaign);
                    }
                }

                if campaigns.iter().any(|campaign| campaign.game_name.eq_ignore_ascii_case(&game_name)) {
                    break;
                }
            }
        }
    }

    println!("get_drop_campaigns: found {} campaigns", campaigns.len());
    Ok(campaigns)
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
        let mut cached_game_id: Option<String> = None;
        let mut cached_streamer_login: Option<String> = None;
        let mut cached_streamer_name: Option<String> = None;
        let mut cached_streamer_image: Option<String> = None;
        let mut cached_stream_id: Option<String> = None;
        let mut cached_channel_id: Option<String> = None;
        let mut cached_spade_url: Option<String> = None;
        
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
                json_id_to_string(ui.pointer("/data/currentUser/id"))
            } else {
                None
            };

            if let Some(uid) = current_user_id {
                println!("start_miner: got user id {}, checking inventory", uid);
                if let Ok(inventory) = gql.get_inventory(&store).await {
                    let in_progress = inventory.pointer("/data/currentUser/inventory/dropCampaignsInProgress")
                        .and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                    let viewer = inventory.pointer("/data/currentUser/inventory/viewerDropCampaigns")
                        .and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                    println!("start_miner: inventory has {} in-progress and {} viewer campaigns", in_progress, viewer);

                    // Check and Claim
                    check_and_claim_drops(&gql, &store, &uid, &inventory).await;

                    // Find Target Game - Check both in-progress and available campaigns
                    let mut target_game_id = None;
                    let mut target_game_name = None;
                    let mut target_drop_name = None;
                    let mut target_drop_image = None;
                    
                    // First check dropCampaignsInProgress
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
                                        target_drop_name = drop.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        target_drop_image = extract_image_url(drop);
                                        break;
                                    }
                                }
                                
                                if has_incomplete {
                                    target_game_id = json_id_to_string(camp.pointer("/game/id"));
                                    target_game_name = camp.pointer("/game/displayName")
                                        .and_then(|v| v.as_str())
                                        .or_else(|| camp.pointer("/game/name").and_then(|v| v.as_str()))
                                        .map(|s| s.to_string());
                                    break;
                                }
                            }
                        }
                    }
                    
                    // If no in-progress drops, check viewerDropCampaigns (available but not started)
                    if target_game_id.is_none() {
                        if let Some(campaigns) = inventory.pointer("/data/currentUser/inventory/viewerDropCampaigns").and_then(|v| v.as_array()) {
                            for camp in campaigns {
                                if let Some(drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                                    // Find an unclaimed drop
                                    let mut has_unclaimed = false;
                                    for drop in drops {
                                        let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
                                        if !is_claimed {
                                            has_unclaimed = true;
                                            target_drop_name = drop.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
                                            target_drop_image = extract_image_url(drop);
                                            break;
                                        }
                                    }
                                    
                                    if has_unclaimed {
                                        target_game_id = json_id_to_string(camp.pointer("/game/id"));
                                        target_game_name = camp.pointer("/game/displayName")
                                            .and_then(|v| v.as_str())
                                            .or_else(|| camp.pointer("/game/name").and_then(|v| v.as_str()))
                                            .map(|s| s.to_string());
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    if let (Some(game_id), Some(game_name)) = (target_game_id, target_game_name) {
                        let reuse_cached_streamer = cached_game_id.as_deref() == Some(&game_id)
                            && cached_streamer_login.is_some()
                            && cached_spade_url.is_some()
                            && cached_stream_id.is_some()
                            && cached_channel_id.is_some();
                        {
                            let mut s_guard = state_clone.lock().await;
                            let mut new_status = s_guard.status.clone();
                            new_status.status = format!("Looking for {} streamer...", game_name);
                            new_status.current_drop = target_drop_name.clone();
                            new_status.drop_image_url = target_drop_image.clone();
                            s_guard.update_and_notify(&app_clone, new_status);
                        }

                        if reuse_cached_streamer {
                            let streamer_login = cached_streamer_login.clone().unwrap();
                            let target_streamer_name = cached_streamer_name.clone();
                            let target_streamer_image = cached_streamer_image.clone();
                            let target_stream_id = cached_stream_id.clone();
                            let target_channel_id = cached_channel_id.clone();

                            {
                                let mut s_guard = state_clone.lock().await;
                                let mut new_status = s_guard.status.clone();
                                new_status.current_streamer = target_streamer_name.clone().or_else(|| Some(streamer_login.clone()));
                                new_status.current_streamer_login = Some(streamer_login.clone());
                                new_status.streamer_image_url = target_streamer_image.clone();
                                new_status.status = format!("Watching {}...", target_streamer_name.as_deref().unwrap_or(&streamer_login));
                                s_guard.update_and_notify(&app_clone, new_status);
                            }

                            let mut loop_count = 15;
                            while loop_count > 0 {
                                {
                                    let s_guard = state_clone.lock().await;
                                    if !s_guard.is_running {
                                        break;
                                    }
                                }

                                if let Ok(inv) = gql.get_inventory(&store).await {
                                    check_and_claim_drops(&gql, &store, &uid, &inv).await;
                                    let active_drops = collect_incomplete_time_based_drops_for_game(&inv, &game_id);
                                    if !active_drops.is_empty() {
                                        let (drop_name, drop_image, overall_progress, cur_mins, req_mins, remaining, drop_infos) =
                                            summarize_active_drops(&active_drops);
                                        println!(
                                            "drop_progress: active={}, current={}m, required={}m, progress={:.1}%, next_recheck_in={}m, drops={:?}",
                                            active_drops.len(),
                                            cur_mins,
                                            req_mins,
                                            overall_progress,
                                            remaining,
                                            active_drops.iter().map(|(id, name, _, cur, req, pct)| format!("{}:{} {}/{} {}%", id, name, cur, req, pct)).collect::<Vec<_>>()
                                        );
                                        let mut s_guard = state_clone.lock().await;
                                        let mut new_status = s_guard.status.clone();
                                        new_status.current_drop = drop_name;
                                        new_status.drop_image_url = drop_image;
                                        new_status.progress = overall_progress;
                                        new_status.active_drops = drop_infos;
                                        new_status.status = if active_drops.len() > 1 {
                                            format!("{} drops: {}m / {}m", active_drops.len(), cur_mins, req_mins)
                                        } else {
                                            format!("Mining: {}m / {}m", cur_mins, req_mins)
                                        };
                                        s_guard.update_and_notify(&app_clone, new_status);
                                        loop_count = loop_count.max(15);
                                    }
                                }

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
                                if let Some(spade_url) = &cached_spade_url {
                                    let _ = gql.send_spade_minute_watched(spade_url, payload).await;
                                } else {
                                    println!("spade_ping: no spade_url cached, skipping ping");
                                }

                                // Update progress from real-time session (more accurate than Inventory)
                                if let Ok(session_res) = gql.get_drop_current_session(&store, &streamer_login).await {
                                    if let Some(session) = session_res.pointer("/data/currentUser/dropCurrentSession") {
                                        let cur = session.get("currentMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                                        let req = session.get("requiredMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                                        let drop_name_rt = session.pointer("/drop/name").and_then(|v| v.as_str());
                                        let drop_img_rt = session.pointer("/drop/benefitEdges/0/benefit/imageAssetURL").and_then(|v| v.as_str());
                                        let drop_id_rt = session.pointer("/drop/id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        println!("drop_session: cur={}m req={}m name={:?}", cur, req, drop_name_rt);
                                        if req > 0 {
                                            let pct = (cur as f64 / req as f64 * 100.0).min(100.0);
                                            let mut s_guard = state_clone.lock().await;
                                            let mut new_status = s_guard.status.clone();
                                            // Update or insert the real-time drop in active_drops
                                            if let Some(existing) = new_status.active_drops.iter_mut().find(|d| d.id == drop_id_rt || drop_name_rt.map_or(false, |n| d.name == n)) {
                                                existing.current_minutes = cur;
                                                existing.progress = pct;
                                            } else if !drop_id_rt.is_empty() {
                                                new_status.active_drops = vec![crate::models::ActiveDropInfo {
                                                    id: drop_id_rt,
                                                    name: drop_name_rt.unwrap_or("Drop").to_string(),
                                                    image_url: drop_img_rt.map(|s| s.to_string()),
                                                    current_minutes: cur,
                                                    required_minutes: req,
                                                    progress: pct,
                                                }];
                                            }
                                            new_status.progress = pct;
                                            new_status.status = format!("Mining: {}m / {}m", cur, req);
                                            s_guard.update_and_notify(&app_clone, new_status);
                                        }
                                    }
                                }

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
                        } else if let Ok(streams) = gql.get_live_streams(&store, &game_id).await {
                            let mut target_streamer_login = None;
                            let mut target_streamer_name = None;
                            let mut target_streamer_image = None;
                            let mut target_stream_id = None;
                            let mut target_channel_id = None;

                            if let Some(edges) = streams.pointer("/data/game/streams/edges").and_then(|v| v.as_array()) {
                                for edge in edges {
                                    let tags = edge.pointer("/node/tags").and_then(|v| v.as_array());
                                    let freeform_tags = edge.pointer("/node/freeformTags").and_then(|v| v.as_array());
                                    let has_drops = tags.map(|t| t.iter().any(|tag| tag.get("id").and_then(|id| id.as_str()) == Some("c2542d6d-cd10-4532-919b-3d19f30a768b"))).unwrap_or(false)
                                        || freeform_tags.map(|t| t.iter().any(|tag| tag.get("name").and_then(|name| name.as_str()) == Some("DropsEnabled"))).unwrap_or(false);
                                    
                                    if has_drops {
                                        target_streamer_login = edge.pointer("/node/broadcaster/login").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        target_streamer_name = edge.pointer("/node/broadcaster/displayName").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        target_streamer_image = edge.pointer("/node/broadcaster/profileImageURL").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        target_stream_id = json_id_to_string(edge.pointer("/node/id"));
                                        target_channel_id = json_id_to_string(edge.pointer("/node/broadcaster/id"));
                                        break;
                                    }
                                }
                            }

                            if let Some(streamer_login) = target_streamer_login {
                                cached_game_id = Some(game_id.clone());
                                cached_streamer_login = Some(streamer_login.clone());
                                cached_streamer_name = target_streamer_name.clone();
                                cached_streamer_image = target_streamer_image.clone();
                                cached_stream_id = target_stream_id.clone();
                                cached_channel_id = target_channel_id.clone();
                                {
                                    let mut s_guard = state_clone.lock().await;
                                    let mut new_status = s_guard.status.clone();
                                    new_status.current_streamer = target_streamer_name.clone().or_else(|| Some(streamer_login.clone()));
                                    new_status.current_streamer_login = Some(streamer_login.clone());
                                    new_status.streamer_image_url = target_streamer_image.clone();
                                    new_status.status = format!("Watching {}...", target_streamer_name.as_deref().unwrap_or(&streamer_login));
                                    s_guard.update_and_notify(&app_clone, new_status);
                                }

                                let _ = gql.get_playback_access_token(&store, &streamer_login).await;
                                
                                let spade_url = match gql.get_spade_url(&streamer_login).await {
                                    Ok(url) => Some(url),
                                    Err(e) => {
                                        println!("drop_miner: spade url unavailable for {}: {}", streamer_login, e);
                                        None
                                    }
                                };
                                cached_spade_url = spade_url.clone();

                                    let mut loop_count = 15; // default watch time before re-check
                                    
                                    while loop_count > 0 {
                                        {
                                            let s_guard = state_clone.lock().await;
                                            if !s_guard.is_running {
                                                break;
                                            }
                                        }

                                        // Update UI Progress from Inventory (all active drops for this game)
                                        if let Ok(inv) = gql.get_inventory(&store).await {
                                            check_and_claim_drops(&gql, &store, &uid, &inv).await;
                                            let active_drops = collect_incomplete_time_based_drops_for_game(&inv, &game_id);
                                            if !active_drops.is_empty() {
                                                let (drop_name, drop_image, overall_progress, cur_mins, req_mins, remaining, drop_infos) =
                                                    summarize_active_drops(&active_drops);
                                                println!(
                                                    "drop_progress: active={}, current={}m, required={}m, progress={:.1}%, next_recheck_in={}m, drops={:?}",
                                                    active_drops.len(),
                                                    cur_mins,
                                                    req_mins,
                                                    overall_progress,
                                                    remaining,
                                                    active_drops.iter().map(|(id, name, _, cur, req, pct)| format!("{}:{} {}/{} {}%", id, name, cur, req, pct)).collect::<Vec<_>>()
                                                );
                                                let mut s_guard = state_clone.lock().await;
                                                let mut new_status = s_guard.status.clone();
                                                new_status.current_drop = drop_name;
                                                new_status.drop_image_url = drop_image;
                                                new_status.progress = overall_progress;
                                                new_status.active_drops = drop_infos;
                                                new_status.status = if active_drops.len() > 1 {
                                                    format!("{} drops: {}m / {}m", active_drops.len(), cur_mins, req_mins)
                                                } else {
                                                    format!("Mining: {}m / {}m", cur_mins, req_mins)
                                                };
                                                s_guard.update_and_notify(&app_clone, new_status);
                                                loop_count = loop_count.max(15);
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
                                        if let Some(spade_url) = &spade_url {
                                            let _ = gql.send_spade_minute_watched(spade_url, payload).await;
                                        } else {
                                            println!("spade_ping: no spade_url, skipping ping");
                                        }

                                        // Update progress from real-time session
                                        if let Ok(session_res) = gql.get_drop_current_session(&store, &streamer_login).await {
                                            if let Some(session) = session_res.pointer("/data/currentUser/dropCurrentSession") {
                                                let cur = session.get("currentMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                                                let req = session.get("requiredMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                                                let drop_name_rt = session.pointer("/drop/name").and_then(|v| v.as_str());
                                                let drop_img_rt = session.pointer("/drop/benefitEdges/0/benefit/imageAssetURL").and_then(|v| v.as_str());
                                                let drop_id_rt = session.pointer("/drop/id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                println!("drop_session: cur={}m req={}m name={:?}", cur, req, drop_name_rt);
                                                if req > 0 {
                                                    let pct = (cur as f64 / req as f64 * 100.0).min(100.0);
                                                    let mut s_guard = state_clone.lock().await;
                                                    let mut new_status = s_guard.status.clone();
                                                    if let Some(existing) = new_status.active_drops.iter_mut().find(|d| d.id == drop_id_rt || drop_name_rt.map_or(false, |n| d.name == n)) {
                                                        existing.current_minutes = cur;
                                                        existing.progress = pct;
                                                    } else if !drop_id_rt.is_empty() {
                                                        new_status.active_drops = vec![crate::models::ActiveDropInfo {
                                                            id: drop_id_rt,
                                                            name: drop_name_rt.unwrap_or("Drop").to_string(),
                                                            image_url: drop_img_rt.map(|s| s.to_string()),
                                                            current_minutes: cur,
                                                            required_minutes: req,
                                                            progress: pct,
                                                        }];
                                                    }
                                                    new_status.progress = pct;
                                                    new_status.status = format!("Mining: {}m / {}m", cur, req);
                                                    s_guard.update_and_notify(&app_clone, new_status);
                                                }
                                            }
                                        }

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
                            } else {
                                cached_game_id = None;
                                cached_streamer_login = None;
                                cached_streamer_name = None;
                                cached_streamer_image = None;
                                cached_stream_id = None;
                                cached_channel_id = None;
                                cached_spade_url = None;
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
                        cached_game_id = None;
                        cached_streamer_login = None;
                        cached_streamer_name = None;
                        cached_streamer_image = None;
                        cached_stream_id = None;
                        cached_channel_id = None;
                        cached_spade_url = None;
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
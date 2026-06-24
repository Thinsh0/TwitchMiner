use crate::api::TwitchGQL;
use crate::models::{ActiveDropInfo, DropCampaign, DropItem, MinerStatus};
use crate::utils::json_id_to_string;
use crate::engine::ViewingEngine;
use crate::models::{WatchRequest, WatchPurpose};
use serde_json::Value;
use rand::Rng;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[tauri::command]
pub async fn get_claimed_drops(app: AppHandle) -> Result<Vec<DropItem>, String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    let claimed_history: Vec<DropItem> = store
        .get("claimed_drops")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    Ok(claimed_history)
}

/// Resolves the drop claim ID from the inventory response.
/// Returns `(claim_id, drop_json_node)` if found, `None` otherwise.
fn resolve_claim_id<'a>(
    inventory: &'a Value,
    drop_id: &str,
    current_user_id: &str,
) -> Option<(String, &'a Value)> {
    // 1. Search completed drops list
    if let Some(drops) = inventory
        .pointer("/data/currentUser/inventory/drops")
        .and_then(|v| v.as_array())
    {
        for drop in drops {
            if json_id_to_string(drop.get("id")).as_deref() == Some(drop_id) {
                let claim_id = json_id_to_string(drop.pointer("/self/dropInstanceID"))
                    .or_else(|| {
                        let camp_id = json_id_to_string(drop.pointer("/campaign/id"))?;
                        Some(format!("{}#{}#{}", current_user_id, camp_id, drop_id))
                    })?;
                return Some((claim_id, drop));
            }
        }
    }

    // 2. Search campaigns in progress / viewer campaign paths
    let campaign_paths = [
        "/data/currentUser/inventory/dropCampaignsInProgress",
        "/data/currentUser/inventory/viewerDropCampaigns",
    ];
    for path in campaign_paths {
        if let Some(campaigns) = inventory.pointer(path).and_then(|v| v.as_array()) {
            for camp in campaigns {
                let camp_id = json_id_to_string(camp.get("id")).unwrap_or_default();
                if let Some(drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                    for drop in drops {
                        if json_id_to_string(drop.get("id")).as_deref() == Some(drop_id) {
                            let claim_id = json_id_to_string(drop.pointer("/self/dropInstanceID"))
                                .or_else(|| {
                                    if camp_id.is_empty() { return None; }
                                    Some(format!("{}#{}#{}", current_user_id, camp_id, drop_id))
                                })?;
                            return Some((claim_id, drop));
                        }
                    }
                }
            }
        }
    }
    None
}

#[tauri::command]
pub async fn claim_drop_cmd(
    app: AppHandle,
    drop_id: String,
) -> Result<Value, String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;

    let gql = TwitchGQL::new();

    let ui_res = gql.get_user_info(&store).await?;
    let current_user_id = json_id_to_string(ui_res.pointer("/data/currentUser/id"))
        .ok_or_else(|| "Failed to get user ID".to_string())?;

    let inventory = gql.get_inventory(&store).await?;

    let (claim_id, drop_node) = resolve_claim_id(&inventory, &drop_id, &current_user_id)
        .ok_or_else(|| format!("Drop with ID {} not found or cannot be claimed.", drop_id))?;

    log::info!("claim_drop_cmd: claiming drop with claim_id = {}", claim_id);
    let claim_res = gql.claim_drop(&store, &claim_id).await?;

    if let Some(mut item) = parse_drop_item(drop_node) {
        item.is_claimed = true;
        item.claimed_at = Some(chrono::Utc::now().to_rfc3339());
        let mut claimed_history: Vec<DropItem> = store
            .get("claimed_drops")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        if !claimed_history.iter().any(|d| d.id == item.id) {
            claimed_history.push(item.clone());
            let _ = store.set(
                "claimed_drops",
                serde_json::to_value(&claimed_history).unwrap(),
            );
            let _ = store.save();
            let _ = app.emit("drop-claimed", item);
        }
    }

    Ok(claim_res)
}

pub struct MinerState {
    pub status: MinerStatus,
    pub is_running: bool,
    pub task: Option<JoinHandle<()>>,
    pub cancel_token: Option<CancellationToken>,
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
            cancel_token: None,
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

impl Default for MinerState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct MineableDropCandidate {
    campaign_id: String,
    campaign_name: String,
    game_id: String,
    game_name: String,
    drop_id: String,
    drop_name: String,
    image_url: Option<String>,
    current_minutes: i64,
    required_minutes: i64,
    remaining_minutes: i64,
    percent: i64,
    end_at: String,
    score: i64,
}

async fn save_claimed_drop(
    store: &Arc<tauri_plugin_store::Store<tauri::Wry>>,
    app: &AppHandle,
    drop: &Value,
    campaign_id: Option<&str>,
) {
    if let Some(mut item) = parse_drop_item(drop) {
        item.is_claimed = true;
        item.claimed_at = Some(chrono::Utc::now().to_rfc3339());
        item.campaign_id = campaign_id.map(|s| s.to_string());
        let mut claimed_history: Vec<DropItem> = store
            .get("claimed_drops")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        if !claimed_history.iter().any(|d| d.id == item.id) {
            claimed_history.push(item.clone());
            let _ = store.set("claimed_drops", serde_json::to_value(&claimed_history).unwrap());
            let _ = store.save();
            let _ = app.emit("drop-claimed", item);
        }
    }
}

async fn check_and_claim_drops(
    gql: &TwitchGQL,
    store: &Arc<tauri_plugin_store::Store<tauri::Wry>>,
    current_user_id: &str,
    inventory: &Value,
    app: &AppHandle,
) {
    // 1. Check Completed Drops List (percentAwarded >= 100)
    if let Some(drops) = inventory
        .pointer("/data/currentUser/inventory/drops")
        .and_then(|v| v.as_array())
    {
        for drop in drops {
            let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
            let pct = drop.get("percentAwarded").and_then(|v| v.as_i64()).unwrap_or(0);

            if pct < 100 {
                continue;
            }
            if is_claimed {
                let camp_id_str = json_id_to_string(drop.pointer("/campaign/id"));
                save_claimed_drop(store, app, drop, camp_id_str.as_deref()).await;
                continue;
            }
            let drop_id = match json_id_to_string(drop.get("id")) {
                Some(id) => id,
                None => continue,
            };
            let claim_id = json_id_to_string(drop.pointer("/self/dropInstanceID"))
                .or_else(|| {
                    let camp_id = json_id_to_string(drop.pointer("/campaign/id"))?;
                    Some(format!("{}#{}#{}", current_user_id, camp_id, drop_id))
                });

            if let Some(c_id) = claim_id {
                match gql.claim_drop(store, &c_id).await {
                    Ok(res) => log::info!("check_and_claim_drops: claim response: {:?}", res),
                    Err(e) => { log::error!("check_and_claim_drops: claim failed for {}: {}", c_id, e); continue; }
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let camp_id_str = json_id_to_string(drop.pointer("/campaign/id"));
                save_claimed_drop(store, app, drop, camp_id_str.as_deref()).await;
            }
        }
    }

    // 2. Check Active Campaigns Progress (currentMinutes >= requiredMinutes)
    if let Some(campaigns) = inventory
        .pointer("/data/currentUser/inventory/dropCampaignsInProgress")
        .and_then(|v| v.as_array())
    {
        for camp in campaigns {
            let camp_id = json_id_to_string(camp.get("id")).unwrap_or_default();
            if let Some(time_drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                for drop in time_drops {
                    let is_claimed = drop.pointer("/self/isClaimed").and_then(|v| v.as_bool()).unwrap_or(false);
                    let cur_mins = drop.pointer("/self/currentMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);
                    let req_mins = drop.get("requiredMinutesWatched").and_then(|v| v.as_i64()).unwrap_or(0);

                    if is_claimed {
                        save_claimed_drop(store, app, drop, if camp_id.is_empty() { None } else { Some(&camp_id) }).await;
                        continue;
                    }
                    if req_mins <= 0 || cur_mins < req_mins {
                        continue;
                    }
                    let drop_id = match json_id_to_string(drop.get("id")) {
                        Some(id) => id,
                        None => continue,
                    };
                    let claim_id = json_id_to_string(drop.pointer("/self/dropInstanceID"))
                        .or_else(|| {
                            if camp_id.is_empty() { return None; }
                            Some(format!("{}#{}#{}", current_user_id, camp_id, drop_id))
                        });

                    if let Some(c_id) = claim_id {
                        match gql.claim_drop(store, &c_id).await {
                            Ok(res) => log::info!("check_and_claim_drops: claim response: {:?}", res),
                            Err(e) => { log::error!("check_and_claim_drops: claim failed for {}: {}", c_id, e); continue; }
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        save_claimed_drop(store, app, drop, if camp_id.is_empty() { None } else { Some(&camp_id) }).await;
                    }
                }
            }
        }
    }
}

fn extract_image_url(drop: &Value) -> Option<String> {
    if let Some(edges) = drop.get("benefitEdges").and_then(|v| v.as_array()) {
        if let Some(first) = edges.first() {
            if let Some(url) = first
                .pointer("/benefit/imageAssetURL")
                .and_then(|v| v.as_str())
            {
                return Some(url.to_string());
            }
        }
    }
    drop.get("imageURL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn parse_drop_item(drop: &Value) -> Option<DropItem> {
    let id = json_id_to_string(drop.get("id"))?;
    let name = drop
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Drop Reward")
        .to_string();
    let required_minutes = drop
        .get("requiredMinutesWatched")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let current_minutes = drop
        .pointer("/self/currentMinutesWatched")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let is_claimed = drop
        .pointer("/self/isClaimed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(DropItem {
        id,
        name,
        required_minutes,
        current_minutes,
        is_claimed,
        image_url: extract_image_url(drop),
        claimed_at: None,
        campaign_id: None,
    })
}

fn parse_campaign_node(camp: &Value, status: &str) -> Option<DropCampaign> {
    let id = json_id_to_string(camp.get("id"))?;
    let name = camp
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Drop Campaign")
        .to_string();
    // Extract the real Twitch game ID (fixes BUG6: was using campaign ID as game ID)
    let game_id = json_id_to_string(camp.pointer("/game/id")).unwrap_or_default();
    let game_name = camp
        .pointer("/game/displayName")
        .and_then(|v| v.as_str())
        .or_else(|| camp.pointer("/game/name").and_then(|v| v.as_str()))
        .unwrap_or("Unknown Game")
        .to_string();
    let image_url = camp
        .pointer("/game/boxArtURL")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let start_date = camp
        .get("startAt")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let end_date = camp
        .get("endAt")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

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
        game_id,
        game_name,
        image_url,
        status: status.to_string(),
        start_date,
        end_date,
        drops,
    })
}

fn parse_all_campaigns(global_campaigns: &Value, inventory: &Value) -> Vec<DropCampaign> {
    let mut parsed: Vec<DropCampaign> = Vec::new();
    let mut by_id: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    if let Some(campaigns) = global_campaigns
        .pointer("/data/currentUser/dropCampaigns")
        .and_then(|v| v.as_array())
    {
        for camp in campaigns {
            let status = camp.get("status").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
            if let Some(campaign) = parse_campaign_node(camp, status) {
                by_id.insert(campaign.id.clone(), parsed.len());
                parsed.push(campaign);
            }
        }
    }

    let campaign_paths = [
        "/data/currentUser/inventory/dropCampaignsInProgress",
        "/data/currentUser/inventory/viewerDropCampaigns",
    ];

    for path in campaign_paths {
        if let Some(campaigns) = inventory.pointer(path).and_then(|v| v.as_array()) {
            for camp in campaigns {
                let camp_id = json_id_to_string(camp.get("id")).unwrap_or_default();
                if let Some(idx) = by_id.get(&camp_id) {
                    if let Some(time_drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                        for drop_json in time_drops {
                            if let Some(parsed_drop) = parse_drop_item(drop_json) {
                                if let Some(target_drop) = parsed[*idx].drops.iter_mut().find(|d| d.id == parsed_drop.id) {
                                    target_drop.current_minutes = parsed_drop.current_minutes;
                                    target_drop.is_claimed = parsed_drop.is_claimed;
                                } else {
                                    parsed[*idx].drops.push(parsed_drop);
                                }
                            }
                        }
                    }
                } else {
                    if let Some(campaign) = parse_campaign_node(camp, "ACTIVE") {
                        by_id.insert(campaign.id.clone(), parsed.len());
                        parsed.push(campaign);
                    }
                }
            }
        }
    }

    parsed
}

/// Returns true if all known drops for a campaign (from the persistent structure cache)
/// are present as claimed in the `claimed_drops` list.
/// Works across restarts and regardless of whether campaign_id was set on the DropItem.
fn is_campaign_completed_via_claimed_drops(
    known_drop_ids: &[String],
    claimed_drops: &[DropItem],
) -> bool {
    if known_drop_ids.is_empty() {
        return false;
    }
    known_drop_ids.iter().all(|drop_id| {
        claimed_drops.iter().any(|d| &d.id == drop_id && d.is_claimed)
    })
}

fn build_mineable_candidates(
    campaigns: &[DropCampaign],
    priority_games: &[String],
    mine_all: bool,
) -> Vec<MineableDropCandidate> {
    let mut candidates = Vec::new();

    for camp in campaigns {
        if camp.id.is_empty() {
            continue;
        }

        if camp.drops.is_empty() {
            if !mine_all && !priority_games.iter().any(|game| game.eq_ignore_ascii_case(&camp.game_name)) {
                continue;
            }
            let mut score = 40;
            if priority_games.iter().any(|game| game.eq_ignore_ascii_case(&camp.game_name)) {
                score += 100;
            }
            candidates.push(MineableDropCandidate {
                campaign_id: camp.id.clone(),
                campaign_name: camp.name.clone(),
                game_id: camp.game_id.clone(),
                game_name: camp.game_name.clone(),
                // Encode campaign_id so the outer loop can track probe results.
                drop_id: format!("PROVISIONAL:{}", camp.id),
                drop_name: String::from("Starting Campaign..."),
                image_url: camp.image_url.clone(),
                current_minutes: 0,
                required_minutes: 15,
                remaining_minutes: 15,
                percent: 0,
                end_at: camp.end_date.clone(),
                score,
            });
            continue;
        }

        for drop in &camp.drops {
            if drop.is_claimed || (drop.required_minutes > 0 && drop.current_minutes >= drop.required_minutes) {
                continue;
            }

            let remaining_minutes = if drop.required_minutes > 0 {
                drop.required_minutes.saturating_sub(drop.current_minutes)
            } else {
                0
            };

            let percent = if drop.required_minutes > 0 {
                (drop.current_minutes * 100 / drop.required_minutes).clamp(0, 100)
            } else {
                0
            };

            let mut score = 0;
            if priority_games.iter().any(|game| game.eq_ignore_ascii_case(&camp.game_name)) {
                score += 100;
            }
            if drop.current_minutes > 0 {
                score += 80;
            }
            score += (100 - remaining_minutes.min(100)).max(0) as i64;
            score += percent.min(100) as i64 / 2;

            if !mine_all && !priority_games.iter().any(|game| game.eq_ignore_ascii_case(&camp.game_name)) {
                continue;
            }

            candidates.push(MineableDropCandidate {
                campaign_id: camp.id.clone(),
                campaign_name: camp.name.clone(),
                // game_id is stored separately from campaign_id; use the game-level id stored in DropCampaign
                game_id: camp.game_id.clone(),
                game_name: camp.game_name.clone(),
                drop_id: drop.id.clone(),
                drop_name: drop.name.clone(),
                image_url: drop.image_url.clone(),
                current_minutes: drop.current_minutes as i64,
                required_minutes: drop.required_minutes as i64,
                remaining_minutes: remaining_minutes as i64,
                percent: percent as i64,
                end_at: camp.end_date.clone(),
                score,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.remaining_minutes.cmp(&b.remaining_minutes))
    });
    candidates
}

fn available_drops_contains_candidate(response: &Value, candidate: &MineableDropCandidate) -> bool {
    let campaigns = response
        .pointer("/data/channel/viewerDropCampaigns")
        .and_then(|v| v.as_array());

    let Some(campaigns) = campaigns else {
        return false;
    };

    campaigns.iter().any(|camp| {
        let campaign_matches = json_id_to_string(camp.get("id"))
            .map(|id| id == candidate.campaign_id)
            .unwrap_or(false);
        if !campaign_matches {
            return false;
        }

        if candidate.drop_id.starts_with("PROVISIONAL") {
            return true;
        }

        camp.get("timeBasedDrops")
            .and_then(|v| v.as_array())
            .map(|drops| {
                drops.iter().any(|drop| {
                    json_id_to_string(drop.get("id"))
                        .map(|id| id == candidate.drop_id)
                        .unwrap_or_else(|| {
                            drop.get("name")
                                .and_then(|v| v.as_str())
                                .map(|name| name == candidate.drop_name)
                                .unwrap_or(false)
                        })
                })
            })
            .unwrap_or(true)
    })
}

/// Look up the live (currentMinutesWatched, requiredMinutesWatched) for a drop_id
/// from a fresh inventory response. Returns None if not found.
fn find_live_drop_progress(inventory: &Value, drop_id: &str) -> Option<(i64, i64)> {
    // Search in dropCampaignsInProgress
    if let Some(campaigns) = inventory
        .pointer("/data/currentUser/inventory/dropCampaignsInProgress")
        .and_then(|v| v.as_array())
    {
        for camp in campaigns {
            if let Some(drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                for drop in drops {
                    if json_id_to_string(drop.get("id")).as_deref() == Some(drop_id) {
                        let cur = drop
                            .pointer("/self/currentMinutesWatched")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let req = drop
                            .get("requiredMinutesWatched")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        return Some((cur, req));
                    }
                }
            }
        }
    }
    // Also search viewerDropCampaigns
    if let Some(campaigns) = inventory
        .pointer("/data/currentUser/inventory/viewerDropCampaigns")
        .and_then(|v| v.as_array())
    {
        for camp in campaigns {
            if let Some(drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                for drop in drops {
                    if json_id_to_string(drop.get("id")).as_deref() == Some(drop_id) {
                        let cur = drop
                            .pointer("/self/currentMinutesWatched")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let req = drop
                            .get("requiredMinutesWatched")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        return Some((cur, req));
                    }
                }
            }
        }
    }
    None
}

fn summarize_active_drops(
    active_drops: &[(String, String, Option<String>, i64, i64, i64)],
) -> (
    Option<String>,
    Option<String>,
    f64,
    i64,
    i64,
    i64,
    Vec<ActiveDropInfo>,
) {
    let total_required: i64 = active_drops.iter().map(|(_, _, _, _, req, _)| *req).sum();
    // Always use cur_mins directly — percentAwarded is unreliable (often 0)
    let total_current: i64 = active_drops.iter().map(|(_, _, _, cur, _, _)| *cur).sum();
    let min_remaining = active_drops
        .iter()
        .map(|(_, _, _, cur, req, _)| (req - cur).max(1))
        .min()
        .unwrap_or(15);
    let overall_progress = if total_required > 0 {
        (total_current as f64 / total_required as f64) * 100.0
    } else {
        0.0
    };
    let current_drop = match active_drops.len() {
        0 => None,
        1 => active_drops
            .first()
            .map(|(_, name, _, _, _, _)| name.clone()),
        count => Some(format!("{} drops actifs en cours", count)),
    };
    let image = active_drops
        .iter()
        .find_map(|(_, _, image, _, _, _)| image.clone());
    let drop_infos: Vec<ActiveDropInfo> = active_drops
        .iter()
        .map(|(id, name, img, cur, req, _)| {
            let pct = if *req > 0 {
                (*cur as f64 / *req as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            ActiveDropInfo {
                id: id.clone(),
                name: name.clone(),
                image_url: img.clone(),
                current_minutes: *cur,
                required_minutes: *req,
                progress: pct,
            }
        })
        .collect();
    (
        current_drop,
        image,
        overall_progress,
        total_current,
        total_required,
        min_remaining,
        drop_infos,
    )
}




#[tauri::command]
pub async fn get_drop_campaigns(app: AppHandle) -> Result<Vec<DropCampaign>, String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    let gql = TwitchGQL::new();
    let global_campaigns = gql.get_all_campaigns(&store).await?;
    let inventory = gql.get_inventory(&store).await?;
    let mut campaigns = parse_all_campaigns(&global_campaigns, &inventory);

    // Persist the campaign → drop_ids structure so we can detect completed campaigns
    // after a restart even when Twitch no longer returns their drops.
    let mut camp_structure: std::collections::HashMap<String, Vec<String>> = store
        .get("campaign_drop_structure")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    for camp in &campaigns {
        if !camp.drops.is_empty() {
            camp_structure.insert(camp.id.clone(), camp.drops.iter().map(|d| d.id.clone()).collect());
        }
    }
    let _ = store.set("campaign_drop_structure", serde_json::to_value(&camp_structure).unwrap());

    let claimed_drops: Vec<DropItem> = store
        .get("claimed_drops")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let now = chrono::Utc::now();
    campaigns.retain(|camp| {
        if let Ok(end_date) = chrono::DateTime::parse_from_rfc3339(&camp.end_date) {
            if now > end_date.with_timezone(&chrono::Utc) {
                return false;
            }
        }
        if camp.status != "ACTIVE" && camp.status != "AVAILABLE" && camp.status != "UPCOMING" {
            return false;
        }
        let all_completed = !camp.drops.is_empty() && camp.drops.iter().all(|d| d.is_claimed || (d.required_minutes > 0 && d.current_minutes >= d.required_minutes));
        if all_completed {
            return false;
        }
        // Campaign came back with no drops: check if we've seen its drop structure before
        // and if all those drop IDs are in claimed_drops.
        if camp.drops.is_empty() {
            if let Some(known_ids) = camp_structure.get(&camp.id) {
                if is_campaign_completed_via_claimed_drops(known_ids, &claimed_drops) {
                    return false;
                }
            }
        }
        true
    });

    let app_config = store
        .get("app_config")
        .and_then(|value| serde_json::from_value::<crate::models::AppConfig>(value).ok())
        .unwrap_or_default();

    let priority_games = app_config.priorities.games;
    let mine_all = app_config.settings.mine_all_inventory;

    if !mine_all {
        campaigns.retain(|c| priority_games.iter().any(|g| g.eq_ignore_ascii_case(&c.game_name)));
    }

    Ok(campaigns)
}


#[derive(Clone)]
struct TargetStream {
    game_id: String,
    streamer_login: String,
    streamer_name: Option<String>,
    streamer_image: Option<String>,
    stream_id: String,
    channel_id: String,
    active_drop: (String, String, Option<String>, i64, i64, i64),
}

#[tauri::command]
pub async fn start_miner(
    app: AppHandle,
    state: tauri::State<'_, Arc<Mutex<MinerState>>>,
    engine: tauri::State<'_, Arc<ViewingEngine>>,
    auto_start_check: Option<bool>,
) -> Result<(), String> {
    let engine = engine.inner().clone();
    let mut s = state.lock().await;
    if s.is_running {
        return Ok(());
    }

    s.is_running = true;
    let cancel_token = CancellationToken::new();
    s.cancel_token = Some(cancel_token.clone());
    let mut new_status = s.status.clone();
    new_status.status = "Starting...".to_string();
    new_status.is_running = true;
    new_status.started_at = Some(chrono::Utc::now().to_rfc3339());
    s.update_and_notify(&app, new_status);

    let state_clone = state.inner().clone();
    let app_clone = app.clone();

    let store = match app.store("config.json") {
        Ok(st) => st,
        Err(e) => return Err(e.to_string()),
    };

    let handle = tokio::spawn(async move {
        let mut current_auto_start = auto_start_check.unwrap_or(false);
        let gql = TwitchGQL::new();
        log::info!("start_miner: background thread started");
        let mut active_streams: Vec<TargetStream> = Vec::new();
        // Cache user_id at startup; refresh only on auth error
        let mut cached_user_id: Option<String> = None;
        // Campaign IDs that returned drops:[] after a probe watch — skipped for this session.
        // Pure RAM: resets every time the miner is started. No disk persistence.
        let exhausted_campaigns: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Campaign IDs from PROVISIONAL streams in the last discovery cycle, waiting to be checked.
        

        loop {
            if cancel_token.is_cancelled() {
                break;
            }
            {
                let s_guard = state_clone.lock().await;
                if !s_guard.is_running {
                    break;
                }
            }

            let app_config = store
                .get("app_config")
                .and_then(|value| {
                    serde_json::from_value::<crate::models::AppConfig>(value).ok()
                })
                .unwrap_or_default();

            if !crate::models::is_mining_allowed(&app_config.schedule) {
                {
                    let mut s_guard = state_clone.lock().await;
                    let mut new_status = s_guard.status.clone();
                    new_status.status = "Suspended by schedule".to_string();
                    new_status.current_streamer = None;
                    new_status.current_streamer_login = None;
                    new_status.current_drop = None;
                    new_status.drop_image_url = None;
                    new_status.streamer_image_url = None;
                    new_status.progress = 0.0;
                    new_status.active_drops = Vec::new();
                    s_guard.update_and_notify(&app_clone, new_status);
                }
                { let jitter = rand::thread_rng().gen_range(1..=10); tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await; }
                continue;
            }

            {
                let mut s_guard = state_clone.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.status = "Checking Configuration...".to_string();
                s_guard.update_and_notify(&app_clone, new_status);
            }

            // P3: Use cached user_id, only re-fetch when missing or auth failed
            if cached_user_id.is_none() {
                let ui_res = gql.get_user_info(&store).await;
                cached_user_id = if let Ok(ui) = &ui_res {
                    json_id_to_string(ui.pointer("/data/currentUser/id"))
                } else {
                    None
                };
            }

            if let Some(uid) = cached_user_id.clone() {
                log::info!(
                    "start_miner: got user id {}, checking configured games",
                    uid
                );

                let app_config_data = store
                    .get("app_config")
                    .and_then(|value| {
                        serde_json::from_value::<crate::models::AppConfig>(value).ok()
                    });
                let priority_games = app_config_data
                    .as_ref()
                    .map(|config| config.priorities.games.clone())
                    .unwrap_or_default();
                let priority_streamers = app_config_data
                    .as_ref()
                    .map(|config| config.priorities.streamers.clone())
                    .unwrap_or_default();

                // Before clearing streams: collect PROVISIONAL campaign IDs from last cycle
                // so we can check them against the freshly fetched campaigns below.


                active_streams.clear();

                let global_campaigns = match gql.get_all_campaigns(&store).await {
                    Ok(val) => val,
                    Err(e) => {
                        log::info!("start_miner: global campaigns error: {}", e);
                        let jitter = rand::thread_rng().gen_range(1u64..=30);
                        tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await;
                        continue;
                    }
                };

                let inventory = match gql.get_inventory(&store).await {
                    Ok(inventory) => {
                        check_and_claim_drops(&gql, &store, &uid, &inventory, &app_clone).await;
                        inventory
                    }
                    Err(e) => {
                        // Auth error: invalidate cached user_id so it will be re-fetched next cycle
                        if e.contains("401") || e.contains("unauthenticated") {
                            cached_user_id = None;
                        }
                        log::info!("start_miner: inventory error: {}", e);
                        let jitter = rand::thread_rng().gen_range(1u64..=30);
                        tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await;
                        continue;
                    }
                };

                let mut campaigns = parse_all_campaigns(&global_campaigns, &inventory);

                // Mark probed-but-still-empty campaigns as exhausted (RAM only, not persisted).


                // Persist campaign → drop_ids structure (miner loop copy)
                let mut camp_structure: std::collections::HashMap<String, Vec<String>> = store
                    .get("campaign_drop_structure")
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                for camp in &campaigns {
                    if !camp.drops.is_empty() {
                        camp_structure.insert(camp.id.clone(), camp.drops.iter().map(|d| d.id.clone()).collect());
                    }
                }
                let _ = store.set("campaign_drop_structure", serde_json::to_value(&camp_structure).unwrap());

                let claimed_drops: Vec<DropItem> = store
                    .get("claimed_drops")
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();

                let now = chrono::Utc::now();
                campaigns.retain(|camp| {
                    if let Ok(end_date) = chrono::DateTime::parse_from_rfc3339(&camp.end_date) {
                        if now > end_date.with_timezone(&chrono::Utc) {
                            return false;
                        }
                    }
                    if camp.status != "ACTIVE" && camp.status != "AVAILABLE" && camp.status != "UPCOMING" {
                        return false;
                    }
                    let all_completed = !camp.drops.is_empty() && camp.drops.iter().all(|d| d.is_claimed || (d.required_minutes > 0 && d.current_minutes >= d.required_minutes));
                    if all_completed {
                        return false;
                    }
                    if camp.drops.is_empty() {
                        // Skip campaigns exhausted by the probe strategy (RAM-only).
                        if exhausted_campaigns.contains(&camp.id) {
                            return false;
                        }
                        if let Some(known_ids) = camp_structure.get(&camp.id) {
                            if is_campaign_completed_via_claimed_drops(known_ids, &claimed_drops) {
                                return false;
                            }
                        }
                    }
                    true
                });

                let mine_all = app_config.settings.mine_all_inventory;

                if !mine_all && priority_games.is_empty() {
                    log::info!("start_miner: No priority games configured.");
                    let mut s_guard = state_clone.lock().await;
                    s_guard.status.status = "No priority games configured.".to_string();
                    
                    if current_auto_start {
                        s_guard.status.status = "Idle (Auto-start: No priority games)".to_string();
                        s_guard.status.is_running = false;
                        s_guard.is_running = false;
                        let s = s_guard.status.clone();
                        s_guard.update_and_notify(&app_clone, s);
                        break;
                    }
                    
                    let s = s_guard.status.clone();
                    s_guard.update_and_notify(&app_clone, s);
                    drop(s_guard);
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                    continue;
                }

                let candidates = build_mineable_candidates(&campaigns, &priority_games, mine_all);

                for candidate in candidates {
                    if active_streams
                        .iter()
                        .any(|stream| stream.game_id == candidate.game_id)
                    {
                        continue;
                    }

                    {
                        let mut s_guard = state_clone.lock().await;
                        let mut new_status = s_guard.status.clone();
                        new_status.status = format!(
                            "Looking for eligible channel: {} / {} ({}) until {}",
                            candidate.game_name,
                            candidate.drop_name,
                            candidate.campaign_name,
                            candidate.end_at
                        );

                        s_guard.update_and_notify(&app_clone, new_status);
                    }

                    let streams = match gql.get_live_streams(&store, &candidate.game_id).await {
                        Ok(value) => value,
                        Err(e) => {
                            log::info!(
                                "start_miner: live streams failed for {}: {}",
                                candidate.game_name, e
                            );
                            continue;
                        }
                    };

                    let Some(edges) = streams
                        .pointer("/data/game/streams/edges")
                        .and_then(|v| v.as_array())
                    else {
                        continue;
                    };

                    let mut sorted_edges = edges.clone();
                    sorted_edges.sort_by_key(|edge| {
                        let login = edge.pointer("/node/broadcaster/login").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                        let display_name = edge.pointer("/node/broadcaster/displayName").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                        let pos = priority_streamers.iter().position(|s| {
                            let s_lower = s.to_lowercase();
                            s_lower == login || s_lower == display_name
                        });
                        match pos {
                            Some(idx) => idx as i64,
                            None => i64::MAX,
                        }
                    });

                    for edge in &sorted_edges {
                        let tags = edge.pointer("/node/tags").and_then(|v| v.as_array());
                        let freeform_tags = edge
                            .pointer("/node/freeformTags")
                            .and_then(|v| v.as_array());
                        let has_drops = tags
                            .map(|t| {
                                t.iter().any(|tag| {
                                    tag.get("id").and_then(|id| id.as_str())
                                        == Some("c2542d6d-cd10-4532-919b-3d19f30a768b")
                                })
                            })
                            .unwrap_or(false)
                            || freeform_tags
                                .map(|t| {
                                    t.iter().any(|tag| {
                                        tag.get("name").and_then(|name| name.as_str())
                                            == Some("DropsEnabled")
                                    })
                                })
                                .unwrap_or(false);

                        // For unstarted campaigns (PROVISIONAL), skip the drops-tag check:
                        // the tag may not yet be set, but watching the stream will activate the campaign.
                        if !has_drops && !candidate.drop_id.starts_with("PROVISIONAL") {
                            continue;
                        }

                        let channel_id = json_id_to_string(edge.pointer("/node/broadcaster/id"))
                            .unwrap_or_default();
                        if channel_id.is_empty() {
                            continue;
                        }

                        let available = match gql.get_available_drops(&store, &channel_id).await {
                            Ok(value) => value,
                            Err(e) => {
                                log::info!(
                                    "start_miner: available drops failed for {}: {}",
                                    candidate.game_name, e
                                );
                                continue;
                            }
                        };

                        if !available_drops_contains_candidate(&available, &candidate) {
                            continue;
                        }

                        if let Some(streamer_login) = edge
                            .pointer("/node/broadcaster/login")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                        {
                            let streamer_name = edge
                                .pointer("/node/broadcaster/displayName")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            let streamer_image = edge
                                .pointer("/node/broadcaster/profileImageURL")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            let stream_id =
                                json_id_to_string(edge.pointer("/node/id")).unwrap_or_default();

                            let _ = gql.get_playback_access_token(&store, &streamer_login).await;

                            active_streams.push(TargetStream {
                                game_id: candidate.game_id.clone(),
                                streamer_login,
                                streamer_name,
                                streamer_image,
                                stream_id,
                                channel_id,
                                active_drop: (
                                    candidate.drop_id.clone(),
                                    candidate.drop_name.clone(),
                                    candidate.image_url.clone(),
                                    candidate.current_minutes,
                                    candidate.required_minutes,
                                    candidate.percent,
                                ),
                            });
                            break;
                        }
                    }

                    if active_streams.len() >= 3 {
                        break;
                    }
                }

                if active_streams.is_empty() {
                    {
                        let mut s_guard = state_clone.lock().await;
                        let mut new_status = s_guard.status.clone();
                        
                        if current_auto_start {
                            new_status.status = "Idle (Auto-start: Nothing to mine)".to_string();
                            new_status.is_running = false;
                            s_guard.is_running = false;
                        } else {
                            new_status.status = "No eligible live channel for active drops.".to_string();
                        }

                        new_status.current_streamer = None;
                        new_status.current_drop = None;
                        new_status.progress = 0.0;
                        new_status.active_drops = Vec::new();
                        s_guard.update_and_notify(&app_clone, new_status);
                    }
                    if current_auto_start {
                        break;
                    }
                    let jitter = rand::thread_rng().gen_range(1u64..=30);
                    tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await;
                    continue;
                }
                
                                // We successfully found a stream to mine, so we are no longer in the startup check
                current_auto_start = false;

                // Submit to engine
                for stream in &active_streams {
                    let req = WatchRequest {
                        channel_login: stream.streamer_login.clone(),
                        channel_id: stream.channel_id.clone(),
                        stream_id: stream.stream_id.clone(),
                        game_id: stream.game_id.clone(),
                        game_name: stream.active_drop.1.clone(), // using drop name as fallback game name
                        purpose: WatchPurpose::Drop {
                            campaign_id: "unknown".to_string(),
                            drop_id: stream.active_drop.0.clone(),
                        },
                        priority: 100,
                    };
                    engine.submit(req).await;
                }

                // Wait before checking next time
                let jitter = rand::thread_rng().gen_range(1u64..=30);
                tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await;
} else {
                // Auth error: clear cached user_id so it will be re-fetched next cycle
                cached_user_id = None;
                {
                    let mut s_guard = state_clone.lock().await;
                    let mut new_status = s_guard.status.clone();
                    new_status.status = "Authentication Error".to_string();
                    s_guard.update_and_notify(&app_clone, new_status);
                }
                let jitter = rand::thread_rng().gen_range(1u64..=30);
                tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await;
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
pub async fn stop_miner(
    app: AppHandle,
    state: tauri::State<'_, Arc<Mutex<MinerState>>>,
) -> Result<(), String> {
    let (task, cancel_token) = {
        let mut s = state.lock().await;
        s.is_running = false;
        let mut new_status = s.status.clone();
        new_status.status = "Stopped".to_string();
        new_status.is_running = false;
        new_status.current_streamer = None;
        new_status.current_streamer_login = None;
        new_status.current_drop = None;
        new_status.drop_image_url = None;
        new_status.streamer_image_url = None;
        new_status.progress = 0.0;
        new_status.active_drops.clear();
        new_status.started_at = None;
        s.update_and_notify(&app, new_status);
        (s.task.take(), s.cancel_token.take())
    };

    // BUG3: Signal cancellation first, then await graceful shutdown
    if let Some(token) = cancel_token {
        token.cancel();
    }
    if let Some(task) = task {
        // Give the task up to 5s to finish cleanly; abort as last resort
        tokio::select! {
            _ = task => {}
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(5)) => {
                log::warn!("stop_miner: task did not finish in time, aborting.");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_find_live_drop_progress() {
        let inventory = json!({
            "data": {
                "currentUser": {
                    "inventory": {
                        "dropCampaignsInProgress": [
                            {
                                "id": "camp1",
                                "timeBasedDrops": [
                                    {
                                        "id": "drop1",
                                        "self": {
                                            "currentMinutesWatched": 45
                                        },
                                        "requiredMinutesWatched": 60
                                    }
                                ]
                            }
                        ],
                        "viewerDropCampaigns": [
                            {
                                "id": "camp2",
                                "timeBasedDrops": [
                                    {
                                        "id": "drop2",
                                        "self": {
                                            "currentMinutesWatched": 10
                                        },
                                        "requiredMinutesWatched": 120
                                    }
                                ]
                            }
                        ]
                    }
                }
            }
        });

        assert_eq!(find_live_drop_progress(&inventory, "drop1"), Some((45, 60)));
        assert_eq!(find_live_drop_progress(&inventory, "drop2"), Some((10, 120)));
        assert_eq!(find_live_drop_progress(&inventory, "non_existent"), None);
    }

    #[test]
    fn test_resolve_claim_id() {
        let inventory = json!({
            "data": {
                "currentUser": {
                    "inventory": {
                        "drops": [
                            {
                                "id": "drop1",
                                "self": {
                                    "dropInstanceID": "instance1"
                                }
                            },
                            {
                                "id": "drop2",
                                "campaign": {
                                    "id": "camp1"
                                }
                            }
                        ],
                        "dropCampaignsInProgress": [
                            {
                                "id": "camp_in_progress",
                                "timeBasedDrops": [
                                    {
                                        "id": "drop_in_progress",
                                        "self": {
                                            "dropInstanceID": "instance_in_progress"
                                        }
                                    }
                                ]
                            }
                        ],
                        "viewerDropCampaigns": [
                            {
                                "id": "camp_viewer",
                                "timeBasedDrops": [
                                    {
                                        "id": "drop_viewer",
                                        "campaign": {
                                            "id": "camp_viewer"
                                        }
                                    }
                                ]
                            }
                        ]
                    }
                }
            }
        });

        // 1. Found in drops list with instance id
        let (id1, _) = resolve_claim_id(&inventory, "drop1", "user123").unwrap();
        assert_eq!(id1, "instance1");

        // 2. Found in drops list with compound id fallback
        let (id2, _) = resolve_claim_id(&inventory, "drop2", "user123").unwrap();
        assert_eq!(id2, "user123#camp1#drop2");

        // 3. Found in progress with instance id
        let (id3, _) = resolve_claim_id(&inventory, "drop_in_progress", "user123").unwrap();
        assert_eq!(id3, "instance_in_progress");

        // 4. Found in viewer drop campaign with compound id fallback
        let (id4, _) = resolve_claim_id(&inventory, "drop_viewer", "user123").unwrap();
        assert_eq!(id4, "user123#camp_viewer#drop_viewer");

        // 5. Not found
        assert!(resolve_claim_id(&inventory, "not_found", "user123").is_none());
    }

    #[test]
    fn test_summarize_active_drops() {
        let active = vec![
            ("camp1".to_string(), "drop1".to_string(), Some("Drop 1".to_string()), 30, 60, 50),
            ("camp1".to_string(), "drop2".to_string(), Some("Drop 2".to_string()), 10, 100, 10),
        ];

        let (curr_drop, _image, progress, total_current, total_required, min_remaining, list) = summarize_active_drops(&active);
        assert_eq!(curr_drop, Some("2 drops actifs en cours".to_string()));
        assert_eq!(progress, 25.0); // (30+10)/(60+100) = 40/160 = 25%
        assert_eq!(total_current, 40);
        assert_eq!(total_required, 160);
        assert_eq!(min_remaining, 30); // min of (60-30)=30 and (100-10)=90 is 30
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_build_mineable_candidates() {
        use crate::models::DropItem;
        
        let campaign_game1 = DropCampaign {
            id: "camp1".to_string(),
            name: "Camp 1".to_string(),
            game_id: "game1_id".to_string(),
            game_name: "Game 1".to_string(),
            end_date: "end_date".to_string(),
            drops: vec![
                DropItem {
                    id: "drop1".to_string(),
                    name: "Drop 1".to_string(),
                    image_url: Some("url1".to_string()),
                    current_minutes: 10,
                    required_minutes: 60,
                    is_claimed: false,
                    ..Default::default()
                },
                DropItem {
                    id: "drop2".to_string(),
                    name: "Drop 2".to_string(),
                    image_url: Some("url2".to_string()),
                    current_minutes: 60,
                    required_minutes: 60,
                    is_claimed: false,
                    ..Default::default()
                }
            ],
            ..Default::default()
        };

        let campaign_game2 = DropCampaign {
            id: "camp2".to_string(),
            name: "Camp 2".to_string(),
            game_id: "game2_id".to_string(),
            game_name: "Game 2".to_string(),
            end_date: "end_date".to_string(),
            drops: vec![
                DropItem {
                    id: "drop3".to_string(),
                    name: "Drop 3".to_string(),
                    image_url: Some("url3".to_string()),
                    current_minutes: 0,
                    required_minutes: 30,
                    is_claimed: false,
                    ..Default::default()
                }
            ],
            ..Default::default()
        };

        let campaigns = vec![campaign_game1, campaign_game2];

        // 1. Mine all: false, Priority game: "Game 1"
        let candidates_pri = build_mineable_candidates(&campaigns, &["Game 1".to_string()], false);
        assert_eq!(candidates_pri.len(), 1);
        assert_eq!(candidates_pri[0].drop_id, "drop1");

        // 2. Mine all: true, Priority game: "Game 1"
        let candidates_all = build_mineable_candidates(&campaigns, &["Game 1".to_string()], true);
        assert_eq!(candidates_all.len(), 2);
        // drop1 should have higher score than drop3 because drop1 has priority game and some minutes watched
        assert_eq!(candidates_all[0].drop_id, "drop1");
        assert_eq!(candidates_all[1].drop_id, "drop3");
    }
}



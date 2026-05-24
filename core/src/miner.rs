use crate::api::TwitchGQL;
use crate::models::{ActiveDropInfo, DropCampaign, DropItem, MinerStatus};
use crate::utils::json_id_to_string;
use serde_json::Value;
use rand::Rng;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

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
    
    let mut resolved_claim_id = None;
    let mut drop_node = None;

    // Search completed drops
    if let Some(drops) = inventory
        .pointer("/data/currentUser/inventory/drops")
        .and_then(|v| v.as_array())
    {
        for drop in drops {
            if let Some(id) = json_id_to_string(drop.get("id")) {
                if id == drop_id {
                    drop_node = Some(drop.clone());
                    let mut claim_id = json_id_to_string(drop.pointer("/self/dropInstanceID"));
                    if claim_id.is_none() {
                        let camp_id = json_id_to_string(drop.pointer("/campaign/id"));
                        if let Some(c) = camp_id {
                            claim_id = Some(format!("{}#{}#{}", current_user_id, c, id));
                        }
                    }
                    resolved_claim_id = claim_id;
                    break;
                }
            }
        }
    }

    // Search campaigns in progress or viewer campaign paths
    if resolved_claim_id.is_none() {
        let campaign_paths = [
            "/data/currentUser/inventory/dropCampaignsInProgress",
            "/data/currentUser/inventory/viewerDropCampaigns",
        ];
        'outer: for path in campaign_paths {
            if let Some(campaigns) = inventory.pointer(path).and_then(|v| v.as_array()) {
                for camp in campaigns {
                    let camp_id = json_id_to_string(camp.get("id")).unwrap_or_default();
                    if let Some(drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                        for drop in drops {
                            if let Some(id) = json_id_to_string(drop.get("id")) {
                                if id == drop_id {
                                    drop_node = Some(drop.clone());
                                    let mut claim_id = json_id_to_string(drop.pointer("/self/dropInstanceID"));
                                    if claim_id.is_none() && !camp_id.is_empty() {
                                        claim_id = Some(format!("{}#{}#{}", current_user_id, camp_id, id));
                                    }
                                    resolved_claim_id = claim_id;
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let claim_id = resolved_claim_id.ok_or_else(|| format!("Drop with ID {} not found or cannot be claimed.", drop_id))?;
    
    log::info!("claim_drop_cmd: claiming drop with claim_id = {}", claim_id);
    let claim_res = gql.claim_drop(&store, &claim_id).await?;

    if let Some(node) = drop_node {
        if let Some(mut item) = parse_drop_item(&node) {
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
    }

    Ok(claim_res)
}

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

async fn check_and_claim_drops(
    gql: &TwitchGQL,
    store: &Arc<tauri_plugin_store::Store<tauri::Wry>>,
    current_user_id: &str,
    inventory: &Value,
    app: &AppHandle,
) {
    // 1. Check Completed Drops List
    if let Some(drops) = inventory
        .pointer("/data/currentUser/inventory/drops")
        .and_then(|v| v.as_array())
    {
        for drop in drops {
            let is_claimed = drop
                .pointer("/self/isClaimed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let pct = drop
                .get("percentAwarded")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

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
                    match gql.claim_drop(store, &c_id).await {
                        Ok(res) => {
                            log::info!("check_and_claim_drops: claim response: {:?}", res);
                        }
                        Err(e) => {
                            log::error!("check_and_claim_drops: claim failed for {}: {}", c_id, e);
                            continue;
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                    if let Some(mut item) = parse_drop_item(drop) {
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
                }
            }
        }
    }

    // 2. Check Active Campaigns Progress
    if let Some(campaigns) = inventory
        .pointer("/data/currentUser/inventory/dropCampaignsInProgress")
        .and_then(|v| v.as_array())
    {
        for camp in campaigns {
            if let Some(time_drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                for drop in time_drops {
                    let is_claimed = drop
                        .pointer("/self/isClaimed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let cur_mins = drop
                        .pointer("/self/currentMinutesWatched")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let req_mins = drop
                        .get("requiredMinutesWatched")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);

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
                            match gql.claim_drop(store, &c_id).await {
                                Ok(res) => {
                                    log::info!("check_and_claim_drops: claim response: {:?}", res);
                                }
                                Err(e) => {
                                    log::error!("check_and_claim_drops: claim failed for {}: {}", c_id, e);
                                    continue;
                                }
                            }
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                            if let Some(mut item) = parse_drop_item(drop) {
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
    })
}

fn parse_campaign_node(camp: &Value, status: &str) -> Option<DropCampaign> {
    let id = json_id_to_string(camp.get("id"))?;
    let name = camp
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Drop Campaign")
        .to_string();
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
    if let Some(campaigns) = inventory
        .pointer("/data/currentUser/inventory/dropCampaignsInProgress")
        .and_then(|v| v.as_array())
    {
        for camp in campaigns {
            if let Some(campaign) = parse_campaign_node(camp, "ACTIVE") {
                by_id.insert(campaign.id.clone(), parsed.len());
                parsed.push(campaign);
            }
        }
    }

    // 2. Available campaigns (not yet started)
    if let Some(campaigns) = inventory
        .pointer("/data/currentUser/inventory/viewerDropCampaigns")
        .and_then(|v| v.as_array())
    {
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
    if let Some(drops) = inventory
        .pointer("/data/currentUser/inventory/drops")
        .and_then(|v| v.as_array())
    {
        for drop in drops {
            let is_claimed = drop
                .pointer("/self/isClaimed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let pct = drop
                .get("percentAwarded")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if !is_claimed && pct < 100 {
                // Not completed yet, already handled by campaign parsing above
                continue;
            }

            let camp_id = json_id_to_string(drop.pointer("/campaign/id")).unwrap_or_default();
            let camp_name = drop
                .pointer("/campaign/name")
                .and_then(|v| v.as_str())
                .unwrap_or("Completed Drops")
                .to_string();
            let game_name = drop
                .pointer("/campaign/game/displayName")
                .and_then(|v| v.as_str())
                .or_else(|| drop.pointer("/campaign/game/name").and_then(|v| v.as_str()))
                .unwrap_or("Unknown Game")
                .to_string();
            let game_image = drop
                .pointer("/campaign/game/boxArtURL")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

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
                        status: if is_claimed {
                            "COMPLETED".to_string()
                        } else {
                            "READY".to_string()
                        },
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

fn build_mineable_candidates(
    inventory: &Value,
    priority_games: &[String],
    mine_all: bool,
) -> Vec<MineableDropCandidate> {
    let mut candidates = Vec::new();
    let campaign_paths = [
        "/data/currentUser/inventory/dropCampaignsInProgress",
        "/data/currentUser/inventory/viewerDropCampaigns",
    ];

    for path in campaign_paths {
        if let Some(campaigns) = inventory.pointer(path).and_then(|v| v.as_array()) {
            for camp in campaigns {
                let campaign_id = json_id_to_string(camp.get("id")).unwrap_or_default();
                let campaign_name = camp
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Drop Campaign")
                    .to_string();
                let game_id = json_id_to_string(camp.pointer("/game/id")).unwrap_or_default();
                let game_name = camp
                    .pointer("/game/displayName")
                    .and_then(|v| v.as_str())
                    .or_else(|| camp.pointer("/game/name").and_then(|v| v.as_str()))
                    .unwrap_or("Unknown Game")
                    .to_string();
                let end_at = camp
                    .get("endAt")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();

                if campaign_id.is_empty() || game_id.is_empty() {
                    continue;
                }

                if let Some(drops) = camp.get("timeBasedDrops").and_then(|v| v.as_array()) {
                    for drop in drops {
                        let is_claimed = drop
                            .pointer("/self/isClaimed")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let current_minutes = drop
                            .pointer("/self/currentMinutesWatched")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let required_minutes = drop
                            .get("requiredMinutesWatched")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        if is_claimed
                            || required_minutes <= 0
                            || current_minutes >= required_minutes
                        {
                            continue;
                        }

                        let drop_id = json_id_to_string(drop.get("id")).unwrap_or_else(|| {
                            drop.get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Drop Reward")
                                .to_string()
                        });
                        let drop_name = drop
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Drop Reward")
                            .to_string();
                        let remaining_minutes = required_minutes - current_minutes;
                        let percent = if required_minutes > 0 {
                            (current_minutes * 100 / required_minutes).clamp(0, 100)
                        } else {
                            drop.get("percentAwarded")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0)
                        };

                        let mut score = 0;
                        if priority_games
                            .iter()
                            .any(|game| game.eq_ignore_ascii_case(&game_name))
                        {
                            score += 100;
                        }
                        if current_minutes > 0 {
                            score += 80;
                        }
                        score += (100 - remaining_minutes.min(100)).max(0);
                        score += percent.min(100) / 2;

                        if !mine_all && !priority_games.iter().any(|game| game.eq_ignore_ascii_case(&game_name)) {
                            continue;
                        }

                        candidates.push(MineableDropCandidate {
                            campaign_id: campaign_id.clone(),
                            campaign_name: campaign_name.clone(),
                            game_id: game_id.clone(),
                            game_name: game_name.clone(),
                            drop_id,
                            drop_name,
                            image_url: extract_image_url(drop),
                            current_minutes,
                            required_minutes,
                            remaining_minutes,
                            percent,
                            end_at: end_at.clone(),
                            score,
                        });
                    }
                }
            }
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

fn parse_available_campaigns(response: &Value, fallback_game_name: &str) -> Vec<DropCampaign> {
    let mut parsed = Vec::new();

    if let Some(campaigns) = response
        .pointer("/data/channel/viewerDropCampaigns")
        .and_then(|v| v.as_array())
    {
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
    campaigns
        .iter()
        .any(|c| !candidate.id.is_empty() && c.id == candidate.id)
}

#[tauri::command]
pub async fn get_drop_campaigns(app: AppHandle) -> Result<Vec<DropCampaign>, String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    let gql = TwitchGQL::new();
    let inventory = gql.get_inventory(&store).await?;
    let mut campaigns = parse_drop_campaigns(&inventory);

    let priority_games = store
        .get("app_config")
        .and_then(|value| serde_json::from_value::<crate::models::AppConfig>(value).ok())
        .map(|config| config.priorities.games)
        .unwrap_or_default();

    for game_name in priority_games {
        let already_has_game = campaigns
            .iter()
            .any(|campaign| campaign.game_name.eq_ignore_ascii_case(&game_name));
        if already_has_game {
            continue;
        }

        log::info!("get_drop_campaigns: checking configured game {}", game_name);
        let search = match gql.search_games(&store, &game_name).await {
            Ok(value) => value,
            Err(e) => {
                log::info!(
                    "get_drop_campaigns: game search failed for {}: {}",
                    game_name, e
                );
                continue;
            }
        };

        let game_id = json_id_to_string(search.pointer("/data/searchFor/games/edges/0/item/id"));
        let Some(ref game_id) = game_id else {
            log::info!("get_drop_campaigns: no game id found for {}", game_name);
            continue;
        };

        let streams = match gql.get_live_streams(&store, game_id).await {
            Ok(value) => value,
            Err(e) => {
                log::info!(
                    "get_drop_campaigns: live streams failed for {}: {}",
                    game_name, e
                );
                continue;
            }
        };

        if let Some(edges) = streams
            .pointer("/data/game/streams/edges")
            .and_then(|v| v.as_array())
        {
            for edge in edges {
                let tags = edge.pointer("/node/tags").and_then(|v| v.as_array());
                let freeform_tags = edge
                    .pointer("/node/freeformTags")
                    .and_then(|v| v.as_array());
                let has_drops_tag = tags
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
                        log::info!(
                            "get_drop_campaigns: available drops failed for {}: {}",
                            game_name, e
                        );
                        continue;
                    }
                };

                let found = parse_available_campaigns(&available, &game_name);
                for campaign in found {
                    if campaign.game_name.eq_ignore_ascii_case(&game_name)
                        && !campaign_exists(&campaigns, &campaign)
                    {
                        campaigns.push(campaign);
                    }
                }

                if campaigns
                    .iter()
                    .any(|campaign| campaign.game_name.eq_ignore_ascii_case(&game_name))
                {
                    break;
                }
            }
        }
    }

    log::info!("get_drop_campaigns: found {} campaigns", campaigns.len());
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
) -> Result<(), String> {
    let mut s = state.lock().await;
    if s.is_running {
        return Ok(());
    }

    s.is_running = true;
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
        let gql = TwitchGQL::new();
        let mut active_streams: Vec<TargetStream> = Vec::new();

        loop {
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

            let ui_res = gql.get_user_info(&store).await;
            let current_user_id = if let Ok(ui) = &ui_res {
                json_id_to_string(ui.pointer("/data/currentUser/id"))
            } else {
                None
            };

            if let Some(uid) = current_user_id {
                log::info!(
                    "start_miner: got user id {}, checking configured games",
                    uid
                );

                let priority_games = store
                    .get("app_config")
                    .and_then(|value| {
                        serde_json::from_value::<crate::models::AppConfig>(value).ok()
                    })
                    .map(|config| config.priorities.games)
                    .unwrap_or_default();

                active_streams.clear();

                let inventory = match gql.get_inventory(&store).await {
                    Ok(inventory) => {
                        check_and_claim_drops(&gql, &store, &uid, &inventory, &app_clone).await;
                        inventory
                    }
                    Err(e) => {
                        log::info!("start_miner: inventory error: {}", e);
                        { let jitter = rand::thread_rng().gen_range(1..=10); tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await; }
                        continue;
                    }
                };

                let mine_all = app_config.settings.mine_all_inventory;

                if !mine_all && priority_games.is_empty() {
                    let mut s_guard = state_clone.lock().await;
                    let mut new_status = s_guard.status.clone();
                    new_status.status = "No config".to_string();
                    s_guard.update_and_notify(&app_clone, new_status);
                    { let jitter = rand::thread_rng().gen_range(1..=10); tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await; }
                    continue;
                }

                let candidates = build_mineable_candidates(&inventory, &priority_games, mine_all);

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

                    for edge in edges {
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

                        if !has_drops {
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
                        new_status.status =
                            "No eligible live channel for active drops.".to_string();

                        new_status.current_streamer = None;
                        new_status.current_drop = None;
                        new_status.progress = 0.0;
                        new_status.active_drops = Vec::new();
                        s_guard.update_and_notify(&app_clone, new_status);
                    }
                    { let jitter = rand::thread_rng().gen_range(1..=10); tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await; }
                    continue;
                }

                // We have active streams, start the mining loop for them
                let mut loop_count = 15;
                while loop_count > 0 {
                    {
                        let s_guard = state_clone.lock().await;
                        if !s_guard.is_running {
                            break;
                        }
                    }

                    // Combine all streams into status
                    let streamers_names: Vec<String> = active_streams
                        .iter()
                        .map(|s| s.streamer_name.clone().unwrap_or(s.streamer_login.clone()))
                        .collect();
                    let status_text = format!("Watching: {}", streamers_names.join(", "));

                    {
                        let mut s_guard = state_clone.lock().await;
                        let mut new_status = s_guard.status.clone();
                        new_status.status = status_text;
                        // Set primary streamer image to first one for UI
                        if let Some(first) = active_streams.first() {
                            new_status.current_streamer = first.streamer_name.clone();
                            new_status.current_streamer_login = Some(first.streamer_login.clone());
                            new_status.streamer_image_url = first.streamer_image.clone();
                        }
                        s_guard.update_and_notify(&app_clone, new_status.clone());
                    }

                    // Collect all active drops
                    if let Ok(inv) = gql.get_inventory(&store).await {
                        check_and_claim_drops(&gql, &store, &uid, &inv, &app_clone).await;
                        let mut all_drops = Vec::new();
                        for stream in &active_streams {
                            all_drops.push(stream.active_drop.clone());
                        }

                        if !all_drops.is_empty() {
                            let (
                                drop_name,
                                drop_image,
                                overall_progress,
                                _cur_mins,
                                _req_mins,
                                _remaining,
                                drop_infos,
                            ) = summarize_active_drops(&all_drops);
                            let mut s_guard = state_clone.lock().await;
                            let mut new_status = s_guard.status.clone();
                            new_status.current_drop = drop_name;
                            new_status.drop_image_url = drop_image;
                            new_status.progress = overall_progress;
                            new_status.active_drops = drop_infos;
                            s_guard.update_and_notify(&app_clone, new_status);
                        } else {
                            let mut s_guard = state_clone.lock().await;
                            let mut new_status = s_guard.status.clone();
                            new_status.active_drops = Vec::new();
                            new_status.progress = 0.0;
                            s_guard.update_and_notify(&app_clone, new_status);
                        }
                    }

                    // Send Pings & Update Session
                    for stream in &active_streams {
                        let client_time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                        let payload = serde_json::json!([{
                            "event": "minute-watched",
                            "properties": {
                                "broadcast_id": stream.stream_id,
                                "channel_id": stream.channel_id,
                                "channel": stream.streamer_login,
                                "client_time": client_time,
                                "game": stream.game_id.clone(),
                                "game_id": stream.game_id.clone(),
                                "hidden": false,
                                "is_live": true,
                                "live": true,
                                "logged_in": true,
                                "minutes_logged": 1,
                                "muted": false,
                                "user_id": uid
                            }
                        }]);

                        let _ = gql.send_watch_gql(&store, payload).await;

                        // Update Realtime Progress
                        if let Ok(session_res) = gql
                            .get_drop_current_session(&store, &stream.streamer_login)
                            .await
                        {
                            if let Some(session) =
                                session_res.pointer("/data/currentUser/dropCurrentSession")
                            {
                                let cur = session
                                    .get("currentMinutesWatched")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);
                                let req = session
                                    .get("requiredMinutesWatched")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);
                                let drop_id_rt = session
                                    .pointer("/drop/id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let drop_name_rt =
                                    session.pointer("/drop/name").and_then(|v| v.as_str());

                                if req > 0 {
                                    let pct = (cur as f64 / req as f64 * 100.0).min(100.0);
                                    let mut s_guard = state_clone.lock().await;
                                    let mut new_status = s_guard.status.clone();
                                    if let Some(existing) =
                                        new_status.active_drops.iter_mut().find(|d| {
                                            d.id == drop_id_rt
                                                || drop_name_rt.map_or(false, |n| d.name == n)
                                        })
                                    {
                                        existing.current_minutes = cur;
                                        existing.progress = pct;
                                    }
                                    s_guard.update_and_notify(&app_clone, new_status);
                                }
                            }
                        }

                        // Claim community points occasionally
                        if loop_count % 5 == 0 {
                            if let Ok(pts_res) = gql
                                .get_channel_points_context(&store, &stream.streamer_login)
                                .await
                            {
                                if let Some(claim_id) = pts_res
                                    .pointer("/data/channel/self/communityPoints/availableClaim/id")
                                    .and_then(|v| v.as_str())
                                {
                                    if let Some(ch_id) =
                                        pts_res.pointer("/data/channel/id").and_then(|v| v.as_str())
                                    {
                                        let _ = gql
                                            .claim_community_points(&store, ch_id, claim_id)
                                            .await;
                                    }
                                }
                            }
                        }
                    }

                    { let jitter = rand::thread_rng().gen_range(1..=10); tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await; }
                    loop_count -= 1;
                }
            } else {
                {
                    let mut s_guard = state_clone.lock().await;
                    let mut new_status = s_guard.status.clone();
                    new_status.status = "Authentication Error".to_string();
                    s_guard.update_and_notify(&app_clone, new_status);
                }
                { let jitter = rand::thread_rng().gen_range(1..=10); tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await; }
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

    if let Some(task) = s.task.take() {
        task.abort();
    }
    Ok(())
}


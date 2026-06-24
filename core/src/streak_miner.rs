use crate::api::TwitchGQL;
use crate::models::ChannelPointsInfo;
use crate::models::StreakMinerStatus;
use crate::utils::json_id_to_string;
use crate::engine::ViewingEngine;
use crate::models::{WatchRequest, WatchPurpose};

use rand::Rng;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub struct StreakMinerState {
    pub status: StreakMinerStatus,
    pub is_running: bool,
    pub task: Option<JoinHandle<()>>,
}

impl StreakMinerState {
    pub fn new() -> Self {
        Self {
            status: StreakMinerStatus {
                status: "Idle".to_string(),
                ..Default::default()
            },
            is_running: false,
            task: None,
        }
    }

    pub fn get_status(&self) -> StreakMinerStatus {
        self.status.clone()
    }

    pub fn update_and_notify(&mut self, app: &AppHandle, new_status: StreakMinerStatus) {
        self.status = new_status.clone();

        // Send real-time update to frontend
        let _ = app.emit("streak-miner-status-update", new_status);
    }
}

#[tauri::command]
pub async fn start_streak_miner(
    app: AppHandle,
    state: tauri::State<'_, Arc<Mutex<StreakMinerState>>>,
    engine: tauri::State<'_, Arc<ViewingEngine>>,
    auto_start_check: Option<bool>,
) -> Result<(), String> {
    let engine = engine.inner().clone();
    let mut s = state.lock().await;
    if s.is_running {
        return Ok(());
    }

    s.is_running = true;
    let mut new_status = s.status.clone();
    new_status.status = "Starting Streak Miner...".to_string();
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

        loop {
            // Check if still running
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
                    new_status.current_channels = Vec::new();
                    new_status.channels_info = Vec::new();
                    s_guard.update_and_notify(&app_clone, new_status);
                }
                { let jitter = rand::thread_rng().gen_range(1..=10); tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await; }
                continue;
            }

            {
                let mut s_guard = state_clone.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.status = "Checking channels configuration...".to_string();
                s_guard.update_and_notify(&app_clone, new_status);
            }

            // Get channels from config (for now, use default channels)
            let channels = get_streak_channels(&store);
            let _mode = get_streak_mode(&store);
            let check_interval = get_check_interval(&store);
            let _max_concurrent = get_max_concurrent(&store);

            if channels.is_empty() {
                if current_auto_start {
                    {
                        let mut s_guard = state_clone.lock().await;
                        let mut new_status = s_guard.status.clone();
                        new_status.status = "Idle (No channels configured)".to_string();
                        s_guard.is_running = false;
                        s_guard.update_and_notify(&app_clone, new_status);
                    }
                    break;
                } else {
                    {
                        let mut s_guard = state_clone.lock().await;
                        let mut new_status = s_guard.status.clone();
                        new_status.status = "No config".to_string();
                        s_guard.update_and_notify(&app_clone, new_status);
                    }
                    for _ in 0..30 {
                        {
                            let s_guard = state_clone.lock().await;
                            if !s_guard.is_running {
                                break;
                            }
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                    continue;
                }
            }

            // Get user info
            let user_info = match gql.get_user_info(&store).await {
                Ok(info) => info,
                Err(e) => {
                    {
                        let mut s_guard = state_clone.lock().await;
                        let mut new_status = s_guard.status.clone();
                        new_status.status = format!("Auth error: {}", e);
                        s_guard.update_and_notify(&app_clone, new_status);
                    }
                    { let jitter = rand::thread_rng().gen_range(1..=10); tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await; }
                    continue;
                }
            };

            let _user_id = match json_id_to_string(user_info.pointer("/data/currentUser/id")) {
                Some(id) => id,
                None => {
                    {
                        let mut s_guard = state_clone.lock().await;
                        let mut new_status = s_guard.status.clone();
                        new_status.status = "Failed to get user ID".to_string();
                        s_guard.update_and_notify(&app_clone, new_status);
                    }
                    { let jitter = rand::thread_rng().gen_range(1..=10); tokio::time::sleep(tokio::time::Duration::from_secs(60 + jitter)).await; }
                    continue;
                }
            };

            // Process channels based on mode
                        // Process channels
            let mut channels_info = Vec::new();
            for channel in &channels {
                channels_info.push(get_channel_points(&gql, &store, channel).await);
            }

            {
                let mut s_guard = state_clone.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.status = "Watching channels...".to_string();
                new_status.current_channels = channels.clone();
                new_status.channels_info = channels_info.clone();
                s_guard.update_and_notify(&app_clone, new_status);
            }

            for channel_name in &channels {
                let stream_info = match gql.get_stream_info(&store, channel_name).await {
                    Ok(info) => info,
                    Err(_) => continue,
                };
                
                if let Some(user_data) = stream_info.pointer("/data/user") {
                    if let Some(stream_data) = user_data.pointer("/stream") {
                        if let Some(channel_id) = json_id_to_string(user_data.get("id")) {
                            if let Some(stream_id) = json_id_to_string(stream_data.get("id")) {
                                let game_name = stream_data.pointer("/game/name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let game_id = json_id_to_string(stream_data.pointer("/game/id")).unwrap_or_default();
                                
                                let req = WatchRequest {
                                    channel_login: channel_name.to_lowercase(),
                                    channel_id: channel_id.clone(),
                                    stream_id,
                                    game_id,
                                    game_name,
                                    purpose: WatchPurpose::ChannelPoints,
                                    priority: 10,
                                };
                                engine.submit(req).await;
                                continue;
                            }
                        }
                    }
                }
                
                // If not live or not found, remove
                if let Some(user_data) = stream_info.pointer("/data/user") {
                    if let Some(channel_id) = json_id_to_string(user_data.get("id")) {
                        engine.remove(&channel_id).await;
                    }
                }
            }

            let any_watching;
            {
                let mut s_guard = state_clone.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.cycle_count += 1;

                any_watching = new_status.channels_info.iter().any(|c| c.status == "Watching");

                if current_auto_start && !any_watching {
                    new_status.status = "Idle (Auto-start: None live)".to_string();
                    s_guard.is_running = false;
                } else {
                    new_status.status = "Waiting for next cycle...".to_string();
                }

                s_guard.update_and_notify(&app_clone, new_status);
            }

            if current_auto_start && !any_watching {
                break;
            }
            
            current_auto_start = false;

            for _ in 0..check_interval {
                {
                    let s_guard = state_clone.lock().await;
                    if !s_guard.is_running {
                        break;
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }

        let mut s_guard = state_clone.lock().await;
        let mut new_status = s_guard.status.clone();
        new_status.status = "Idle".to_string();
        s_guard.update_and_notify(&app_clone, new_status);
    });

    s.task = Some(handle);
    Ok(())
}

#[tauri::command]
pub async fn stop_streak_miner(
    app: AppHandle,
    state: tauri::State<'_, Arc<Mutex<StreakMinerState>>>,
) -> Result<(), String> {
    let mut s = state.lock().await;
    s.is_running = false;
    let mut new_status = s.status.clone();
    new_status.status = "Stopped".to_string();
    new_status.started_at = None;
    s.update_and_notify(&app, new_status);

    if let Some(task) = s.task.take() {
        task.abort();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_streak_miner_status(
    state: tauri::State<'_, Arc<Mutex<StreakMinerState>>>,
) -> Result<StreakMinerStatus, String> {
    let s = state.lock().await;
    Ok(s.get_status())
}

async fn get_channel_points(
    gql: &TwitchGQL,
    store: &Arc<tauri_plugin_store::Store<tauri::Wry>>,
    channel: &str,
) -> ChannelPointsInfo {
    let mut info = ChannelPointsInfo {
        channel_name: channel.to_string(),
        channel_login: channel.to_lowercase(),
        points: 0,
        has_claim_available: false,
        status: "Checking...".to_string(),
        profile_image_url: None,
    };

    match gql.get_channel_points_context(store, channel).await {
        Ok(response) => {
            if let Some(img_url) = response.pointer("/data/user/profileImageURL").and_then(|v| v.as_str()) {
                info.profile_image_url = Some(img_url.to_string());
            }
            if let Some(channel_data) = response.pointer("/data/channel") {
                if let Some(display_name) = channel_data.get("displayName").and_then(|v| v.as_str())
                {
                    info.channel_name = display_name.to_string();
                }
                if let Some(self_data) = channel_data.get("self") {
                    if let Some(points_data) = self_data.get("communityPoints") {
                        info.points = points_data
                            .get("balance")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        info.has_claim_available = points_data.get("availableClaim").is_some();

                        if let Some(claim_id) = points_data
                            .pointer("/availableClaim/id")
                            .and_then(|v| v.as_str())
                        {
                            if let Some(channel_id) =
                                channel_data.get("id").and_then(|v| v.as_str())
                            {
                                match gql
                                    .claim_community_points(store, channel_id, claim_id)
                                    .await
                                {
                                    Ok(_) => {
                                        info.has_claim_available = false;
                                        info.status = "Bonus claimed".to_string();
                                    }
                                    Err(e) => {
                                        log::info!(
                                            "Failed to claim points bonus for {}: {}",
                                            channel, e
                                        );
                                        info.status = "Claim failed".to_string();
                                    }
                                }
                            }
                        }
                    }
                }
                if info.status == "Checking..." {
                    info.status = "Ready".to_string();
                }
            } else {
                info.status = "Channel not found".to_string();
            }
        }
        Err(e) => {
            log::info!("Failed to get points for {}: {}", channel, e);
            info.status = "Error".to_string();
        }
    }

    info
}

fn get_streak_channels(store: &Arc<tauri_plugin_store::Store<tauri::Wry>>) -> Vec<String> {
    if let Some(config_json) = store.get("app_config") {
        if let Ok(config) = serde_json::from_value::<crate::models::AppConfig>(config_json) {
            let mut channels = config.bonus.points_farming.channels;
            for channel in config.bonus.daily_streaks.channels {
                if !channels.iter().any(|c| c.eq_ignore_ascii_case(&channel)) {
                    channels.push(channel);
                }
            }
            return channels;
        }
    }

    // Return empty list if no channels are configured
    vec![]
}

fn get_streak_mode(store: &Arc<tauri_plugin_store::Store<tauri::Wry>>) -> String {
    // Try to load from config
    if let Some(config_json) = store.get("app_config") {
        if let Ok(config) = serde_json::from_value::<crate::models::AppConfig>(config_json) {
            return config.bonus.daily_streaks.mode;
        }
    }

    // Fallback to default
    "sequential".to_string()
}

fn get_check_interval(store: &Arc<tauri_plugin_store::Store<tauri::Wry>>) -> u64 {
    if let Some(config_json) = store.get("app_config") {
        if let Ok(config) = serde_json::from_value::<crate::models::AppConfig>(config_json) {
            return config.settings.check_interval.clamp(10, 300);
        }
    }
    60
}

fn get_max_concurrent(store: &Arc<tauri_plugin_store::Store<tauri::Wry>>) -> usize {
    if let Some(config_json) = store.get("app_config") {
        if let Ok(config) = serde_json::from_value::<crate::models::AppConfig>(config_json) {
            return config.bonus.points_farming.max_concurrent.clamp(1, 10) as usize;
        }
    }
    3
}


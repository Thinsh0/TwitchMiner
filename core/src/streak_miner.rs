use crate::api::TwitchGQL;
use crate::models::ChannelPointsInfo;
use crate::models::StreakMinerStatus;
use crate::utils::json_id_to_string;

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
    auto_start_check: Option<bool>,
) -> Result<(), String> {
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
            let mode = get_streak_mode(&store);
            let check_interval = get_check_interval(&store);
            let max_concurrent = get_max_concurrent(&store);

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

            let user_id = match json_id_to_string(user_info.pointer("/data/currentUser/id")) {
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
            match mode.as_str() {
                "parallel" => {
                    process_channels_parallel(
                        &gql,
                        &store,
                        &channels,
                        &user_id,
                        &state_clone,
                        &app_clone,
                        max_concurrent,
                    )
                    .await
                }
                _ => {
                    process_channels_sequential(
                        &gql,
                        &store,
                        &channels,
                        &user_id,
                        &state_clone,
                        &app_clone,
                    )
                    .await
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

async fn process_channels_sequential(
    gql: &TwitchGQL,
    store: &Arc<tauri_plugin_store::Store<tauri::Wry>>,
    channels: &[String],
    user_id: &str,
    state: &Arc<Mutex<StreakMinerState>>,
    app: &AppHandle,
) {
    // First pass: get points for all channels
    let mut channels_info = Vec::new();
    for channel in channels {
        let info = get_channel_points(gql, store, channel).await;
        channels_info.push(info);

        // Update status with current info
        {
            let mut s_guard = state.lock().await;
            let mut new_status = s_guard.status.clone();
            new_status.status = format!("Fetching points for {}...", channel);
            new_status.channels_info = channels_info.clone();
            s_guard.update_and_notify(app, new_status);
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    // Second pass: send minute-watched pings
    for channel in channels {
        // Check if still running
        {
            let s_guard = state.lock().await;
            if !s_guard.is_running {
                break;
            }
        }

        {
            let mut s_guard = state.lock().await;
            let mut new_status = s_guard.status.clone();
            new_status.status = format!("Watching {}...", channel);
            new_status.current_channels = vec![channel.clone()];
            s_guard.update_and_notify(app, new_status);
        }

        match process_single_channel(gql, store, channel, user_id).await {
            Ok(success) => {
                let mut s_guard = state.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.total_pings_sent += 1;
                if success {
                    new_status.successful_pings += 1;
                } else {
                    new_status.failed_pings += 1;
                }
                // Update channel status in channels_info
                for info in &mut new_status.channels_info {
                    if info.channel_name.to_lowercase() == channel.to_lowercase() {
                        info.status = if success { "Watching" } else { "Ping failed" }.to_string();
                    }
                }
                s_guard.update_and_notify(app, new_status);
            }
            Err(e) => {
                let error_msg = e.clone();
                let mut s_guard = state.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.total_pings_sent += 1;
                new_status.failed_pings += 1;
                new_status.status = format!("Error on {}: {}", channel, e);
                // Update channel status
                for info in &mut new_status.channels_info {
                    if info.channel_name.to_lowercase() == channel.to_lowercase() {
                        info.status = if error_msg.contains("offline") {
                            "Offline"
                        } else {
                            "Error"
                        }
                        .to_string();
                    }
                }
                s_guard.update_and_notify(app, new_status);
            }
        }

        // Small delay between channels to avoid rate limits
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}

async fn process_channels_parallel(
    gql: &TwitchGQL,
    store: &Arc<tauri_plugin_store::Store<tauri::Wry>>,
    channels: &[String],
    user_id: &str,
    state: &Arc<Mutex<StreakMinerState>>,
    app: &AppHandle,
    max_concurrent: usize,
) {
    let mut channels_info = Vec::new();
    for channel in channels {
        channels_info.push(get_channel_points(gql, store, channel).await);
    }

    {
        let mut s_guard = state.lock().await;
        let mut new_status = s_guard.status.clone();
        new_status.status = "Processing channels in parallel...".to_string();
        new_status.current_channels = channels.iter().take(max_concurrent).cloned().collect();
        new_status.channels_info = channels_info;
        s_guard.update_and_notify(app, new_status);
    }

    let mut tasks = Vec::new();

    for channel in channels.iter().take(max_concurrent) {
        let gql = Arc::new(TwitchGQL::new()); // Create new instance for each task
        let store = store.clone();
        let channel = channel.clone();
        let user_id = user_id.to_string();
        let _state = state.clone();
        let _app = app.clone();

        let task =
            tokio::spawn(
                async move { process_single_channel(&*gql, &store, &channel, &user_id).await },
            );

        tasks.push(task);
    }

    // Wait for all tasks and collect results
    for (i, task) in tasks.into_iter().enumerate() {
        let channel_name = channels.get(i).cloned().unwrap_or_default();
        match task.await {
            Ok(Ok(success)) => {
                let mut s_guard = state.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.total_pings_sent += 1;
                if success {
                    new_status.successful_pings += 1;
                } else {
                    new_status.failed_pings += 1;
                }
                for info in &mut new_status.channels_info {
                    if info.channel_name.to_lowercase() == channel_name.to_lowercase() {
                        info.status = if success { "Watching" } else { "Ping failed" }.to_string();
                    }
                }
                s_guard.update_and_notify(app, new_status);
            }
            Ok(Err(e)) => {
                let error_msg = e.clone();
                let mut s_guard = state.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.total_pings_sent += 1;
                new_status.failed_pings += 1;
                for info in &mut new_status.channels_info {
                    if info.channel_name.to_lowercase() == channel_name.to_lowercase() {
                        info.status = if error_msg.contains("offline") {
                            "Offline"
                        } else {
                            "Error"
                        }
                        .to_string();
                    }
                }
                s_guard.update_and_notify(app, new_status);
            }
            Err(e) => {
                log::warn!("[parallel] Task {} panicked: {}", i, e);
                let mut s_guard = state.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.total_pings_sent += 1;
                new_status.failed_pings += 1;
                for info in &mut new_status.channels_info {
                    if info.channel_name.to_lowercase() == channel_name.to_lowercase() {
                        info.status = "Error".to_string();
                    }
                }
                s_guard.update_and_notify(app, new_status);
            }
        }
    }

}

async fn process_single_channel(
    gql: &TwitchGQL,
    store: &Arc<tauri_plugin_store::Store<tauri::Wry>>,
    channel: &str,
    user_id: &str,
) -> Result<bool, String> {
    log::debug!("[process_single_channel] Starting {}", channel);

    // (get_spade_url logic removed because we now use send_watch_gql)

    // Get stream info
    let stream_info = match gql.get_stream_info(store, channel).await {
        Ok(info) => info,
        Err(e) => {
            log::debug!("[process_single_channel] {}: Stream info error: {}", channel, e);
            return Ok(false);
        }
    };

    let user_data = match stream_info.pointer("/data/user") {
        Some(u) => u,
        None => {
            return Ok(false);
        }
    };

    let stream_data = match user_data.pointer("/stream") {
        Some(s) => s,
        None => {
            return Err("Stream is offline".to_string());
        }
    };

    let broadcast_id = match json_id_to_string(stream_data.get("id")) {
        Some(id) => id,
        None => {
            return Ok(false);
        }
    };

    let channel_id = match json_id_to_string(user_data.get("id")) {
        Some(id) => id,
        None => {
            return Ok(false);
        }
    };

    let game_name = stream_data
        .pointer("/game/name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let game_id = json_id_to_string(stream_data.pointer("/game/id")).unwrap_or_default();

    log::debug!(
        "[process_single_channel] {}: broadcast_id={}, channel_id={}, game={}, game_id={}",
        channel, broadcast_id, channel_id, game_name, game_id
    );

    // Prepare and send minute-watched payload
    let client_time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let payload = serde_json::json!([{
        "event": "minute-watched",
        "properties": {
            "broadcast_id": broadcast_id,
            "channel_id": channel_id,
            "channel": channel.to_lowercase(),
            "client_time": client_time,
            "game": game_name,
            "game_id": game_id,
            "hidden": false,
            "is_live": true,
            "live": true,
            "logged_in": true,
            "minutes_logged": 1,
            "muted": false,
            "user_id": user_id,
        }
    }]);

    let result = gql.send_watch_gql(store, payload).await;
    match &result {
        Ok(true) => log::info!("[process_single_channel] {}: Ping OK", channel),
        Ok(false) => log::warn!("[process_single_channel] {}: Ping returned non-success", channel),
        Err(e) => log::error!("[process_single_channel] {}: Ping error: {}", channel, e),
    }
    result
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


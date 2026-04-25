use crate::api::TwitchGQL;
use crate::models::StreakMinerStatus;
use crate::models::ChannelPointsInfo;
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;
use std::sync::Arc;
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
    state: tauri::State<'_, Arc<Mutex<StreakMinerState>>>
) -> Result<(), String> {
    let mut s = state.lock().await;
    if s.is_running {
        return Ok(());
    }

    s.is_running = true;
    let mut new_status = s.status.clone();
    new_status.status = "Starting Streak Miner...".to_string();
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
                new_status.status = "Checking channels configuration...".to_string();
                s_guard.update_and_notify(&app_clone, new_status);
            }

            // Get channels from config (for now, use default channels)
            let channels = get_streak_channels(&store);
            let mode = get_streak_mode(&store);

            if channels.is_empty() {
                {
                    let mut s_guard = state_clone.lock().await;
                    let mut new_status = s_guard.status.clone();
                    new_status.status = "No channels configured for streak mining".to_string();
                    s_guard.update_and_notify(&app_clone, new_status);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                continue;
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
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                    continue;
                }
            };

            let user_id = match user_info.pointer("/data/currentUser/id").and_then(|v| v.as_str()) {
                Some(id) => id,
                None => {
                    {
                        let mut s_guard = state_clone.lock().await;
                        let mut new_status = s_guard.status.clone();
                        new_status.status = "Failed to get user ID".to_string();
                        s_guard.update_and_notify(&app_clone, new_status);
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                    continue;
                }
            };

            // Process channels based on mode
            match mode.as_str() {
                "parallel" => process_channels_parallel(&gql, &store, &channels, user_id, &state_clone, &app_clone).await,
                _ => process_channels_sequential(&gql, &store, &channels, user_id, &state_clone, &app_clone).await,
            }

            // Update cycle count and wait for next cycle
            {
                let mut s_guard = state_clone.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.cycle_count += 1;
                new_status.status = "Waiting for next cycle...".to_string();
                s_guard.update_and_notify(&app_clone, new_status);
            }

            // Wait 60 seconds before next cycle (minute-watched interval)
            for _ in 0..60 {
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
    state: tauri::State<'_, Arc<Mutex<StreakMinerState>>>
) -> Result<(), String> {
    let mut s = state.lock().await;
    s.is_running = false;
    let mut new_status = s.status.clone();
    new_status.status = "Stopped".to_string();
    s.update_and_notify(&app, new_status);
    
    if let Some(task) = s.task.take() {
        task.abort();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_streak_miner_status(state: tauri::State<'_, Arc<Mutex<StreakMinerState>>>) -> Result<StreakMinerStatus, String> {
    let s = state.lock().await;
    Ok(s.get_status())
}

async fn get_channel_points(gql: &TwitchGQL, store: &Arc<tauri_plugin_store::Store<tauri::Wry>>, channel: &str) -> ChannelPointsInfo {
    let mut info = ChannelPointsInfo {
        channel_name: channel.to_string(),
        points: 0,
        has_claim_available: false,
        status: "Checking...".to_string(),
    };

    match gql.get_channel_points_context(store, channel).await {
        Ok(response) => {
            if let Some(channel_data) = response.pointer("/data/channel") {
                if let Some(display_name) = channel_data.get("displayName").and_then(|v| v.as_str()) {
                    info.channel_name = display_name.to_string();
                }
                if let Some(self_data) = channel_data.get("self") {
                    if let Some(points_data) = self_data.get("communityPoints") {
                        info.points = points_data.get("balance")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        info.has_claim_available = points_data.get("availableClaim").is_some();
                    }
                }
                info.status = "Ready".to_string();
            } else {
                info.status = "Channel not found".to_string();
            }
        }
        Err(e) => {
            println!("Failed to get points for {}: {}", channel, e);
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
                        info.status = if error_msg.contains("offline") { "Offline" } else { "Error" }.to_string();
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
    _gql: &TwitchGQL,
    store: &Arc<tauri_plugin_store::Store<tauri::Wry>>,
    channels: &[String],
    user_id: &str,
    state: &Arc<Mutex<StreakMinerState>>,
    app: &AppHandle,
) {
    {
        let mut s_guard = state.lock().await;
        let mut new_status = s_guard.status.clone();
        new_status.status = "Processing channels in parallel...".to_string();
        new_status.current_channels = channels.to_vec();
        s_guard.update_and_notify(app, new_status);
    }

    let mut tasks = Vec::new();
    
    for channel in channels {
        let gql = Arc::new(TwitchGQL::new()); // Create new instance for each task
        let store = store.clone();
        let channel = channel.clone();
        let user_id = user_id.to_string();
        let _state = state.clone();
        let _app = app.clone();

        let task = tokio::spawn(async move {
            process_single_channel(&*gql, &store, &channel, &user_id).await
        });
        
        tasks.push(task);
    }

    // Wait for all tasks and collect results
    for (i, task) in tasks.into_iter().enumerate() {
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
                s_guard.update_and_notify(app, new_status);
            }
            Ok(Err(e)) => {
                let mut s_guard = state.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.total_pings_sent += 1;
                new_status.failed_pings += 1;
                new_status.status = format!("Error in parallel task {}: {}", i, e);
                s_guard.update_and_notify(app, new_status);
            }
            Err(e) => {
                let mut s_guard = state.lock().await;
                let mut new_status = s_guard.status.clone();
                new_status.total_pings_sent += 1;
                new_status.failed_pings += 1;
                new_status.status = format!("Task panicked: {}", e);
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
    println!("Processing channel: {}", channel);
    
    // Get Spade URL
    let spade_url = gql.get_spade_url(channel).await?;
    println!("Got Spade URL for {}: {}...", channel, &spade_url[..spade_url.len().min(50)]);
    
    // Get stream info
    let stream_info = gql.get_stream_info(store, channel).await?;
    println!("Got stream info for {}", channel);
    
    let user_data = stream_info.pointer("/data/user")
        .ok_or("No user data found")?;
    
    let stream_data = user_data.pointer("/stream")
        .ok_or("Stream is offline")?;
    
    let broadcast_id = stream_data.get("id")
        .and_then(|v| v.as_str())
        .ok_or("No broadcast ID")?;
    
    let channel_id = user_data.get("id")
        .and_then(|v| v.as_str())
        .ok_or("No channel ID")?;

    println!("Channel {}: broadcast_id={}, channel_id={}", channel, broadcast_id, channel_id);

    // Prepare and send minute-watched payload
    let payload = serde_json::json!({
        "event": "minute-watched",
        "properties": {
            "broadcast_id": broadcast_id,
            "channel_id": channel_id,
            "channel": channel,
            "hidden": false,
            "live": true,
            "location": "channel",
            "logged_in": true,
            "muted": false,
            "player": "site",
            "user_id": user_id,
        }
    });

    let result = gql.send_spade_minute_watched(&spade_url, payload).await;
    match &result {
        Ok(true) => println!("Ping sent successfully for {}", channel),
        Ok(false) => println!("Ping returned non-success for {}", channel),
        Err(e) => println!("Ping failed for {}: {}", channel, e),
    }
    result
}

fn get_streak_channels(store: &Arc<tauri_plugin_store::Store<tauri::Wry>>) -> Vec<String> {
    // Try to load from config
    if let Some(config_json) = store.get("app_config") {
        if let Ok(config) = serde_json::from_value::<crate::models::AppConfig>(config_json) {
            return config.bonus.daily_streaks.channels;
        }
    }
    
    // Fallback to defaults
    vec![
        "shroud".to_string(),
        "summit1g".to_string(),
        "lirik".to_string(),
        "pokimane".to_string(),
        "ninja".to_string(),
    ]
}

fn get_streak_mode(store: &Arc<tauri_plugin_store::Store<tauri::Wry>>) -> String {
    // Try to load from config
    if let Some(config_json) = store.get("app_config") {
        if let Ok(config) = serde_json::from_value::<crate::models::AppConfig>(config_json) {
            return config.bonus.daily_streaks.mode;
        }
    }
    
    // Fallback to default
    "parallel".to_string()
}

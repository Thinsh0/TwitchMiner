use crate::api::TwitchGQL;
use crate::models::{WatchedChannel, WatchRequest};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::{Store, StoreExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct ViewingEngineInner {
    pub watched: Vec<WatchedChannel>,
    pub client_session_id: String,
    pub max_concurrent: usize,
}

pub struct ViewingEngine {
    pub inner: Arc<Mutex<ViewingEngineInner>>,
}

impl ViewingEngine {
    pub fn new(max_concurrent: usize) -> Self {
        let client_session_id = Uuid::new_v4().to_string().replace("-", "");
        Self {
            inner: Arc::new(Mutex::new(ViewingEngineInner {
                watched: Vec::new(),
                client_session_id,
                max_concurrent,
            })),
        }
    }

    pub async fn submit(&self, request: WatchRequest) -> bool {
        let mut inner = self.inner.lock().await;

        if let Some(existing) = inner.watched.iter_mut().find(|w| w.request.channel_id == request.channel_id) {
            existing.request.purpose = request.purpose;
            return true;
        }

        if inner.watched.len() >= inner.max_concurrent {
            return false;
        }

        let player_session_id = Uuid::new_v4().to_string().replace("-", "");
        let index = inner.watched.len() as u64;
        let ping_offset_secs = if inner.max_concurrent > 0 {
            (index * 60) / (inner.max_concurrent as u64)
        } else {
            0
        };

        let watched_channel = WatchedChannel {
            request,
            player_session_id,
            started_at: Instant::now(),
            last_ping_at: None,
            ping_offset_secs,
        };

        inner.watched.push(watched_channel);
        true
    }

    pub async fn remove(&self, channel_id: &str) {
        let mut inner = self.inner.lock().await;
        inner.watched.retain(|w| w.request.channel_id != channel_id);
    }

    pub async fn get_channels_due_for_ping(&self, now: Instant) -> Vec<WatchedChannel> {
        let inner = self.inner.lock().await;
        let mut due = Vec::new();
        for channel in &inner.watched {
            let is_due = match channel.last_ping_at {
                None => now.duration_since(channel.started_at).as_secs() >= channel.ping_offset_secs,
                Some(last) => now.duration_since(last).as_secs() >= 60,
            };
            if is_due {
                due.push(channel.clone());
            }
        }
        due
    }

    pub async fn mark_pinged(&self, channel_id: &str, now: Instant) {
        let mut inner = self.inner.lock().await;
        if let Some(channel) = inner.watched.iter_mut().find(|w| w.request.channel_id == channel_id) {
            channel.last_ping_at = Some(now);
        }
    }

    pub async fn get_client_session_id(&self) -> String {
        let inner = self.inner.lock().await;
        inner.client_session_id.clone()
    }
}

pub fn build_watch_payload(
    channel: &WatchedChannel,
    user_id: &str,
    client_session_id: &str,
) -> Value {
    let client_time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    json!([{
        "event": "minute-watched",
        "properties": {
            "broadcast_id": channel.request.stream_id,
            "channel_id": channel.request.channel_id,
            "channel": channel.request.channel_login,
            "client_time": client_time,
            "game": channel.request.game_id,
            "game_id": channel.request.game_id,
            "hidden": false,
            "is_live": true,
            "live": true,
            "logged_in": true,
            "minutes_logged": 1,
            "muted": false,
            "user_id": user_id,
            "client_session_id": client_session_id,
            "player_session_id": channel.player_session_id,
            "player_type": "site"
        }
    }])
}

pub fn build_video_play_payload(
    channel: &WatchedChannel,
    user_id: &str,
    client_session_id: &str,
) -> Value {
    let client_time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    json!([{
        "event": "video-play",
        "properties": {
            "broadcast_id": channel.request.stream_id,
            "channel_id": channel.request.channel_id,
            "channel": channel.request.channel_login,
            "client_time": client_time,
            "game": channel.request.game_id,
            "game_id": channel.request.game_id,
            "hidden": false,
            "is_live": true,
            "live": true,
            "logged_in": true,
            "minutes_logged": 1,
            "muted": false,
            "user_id": user_id,
            "client_session_id": client_session_id,
            "player_session_id": channel.player_session_id,
            "player_type": "site"
        }
    }])
}

impl ViewingEngine {
    pub async fn run(
        self: Arc<Self>,
        gql: Arc<TwitchGQL>,
        store: Arc<Store<tauri::Wry>>,
        user_id: String,
        app: AppHandle,
        cancel_token: CancellationToken,
    ) {
        let mut loop_count = 0u64;
        let client_session_id = self.get_client_session_id().await;

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let now = Instant::now();
            let due_channels = self.get_channels_due_for_ping(now).await;

            for channel in due_channels {
                // Determine if this is the first ping (video-play)
                let is_first_ping = channel.last_ping_at.is_none();
                
                let payload = if is_first_ping {
                    build_video_play_payload(&channel, &user_id, &client_session_id)
                } else {
                    build_watch_payload(&channel, &user_id, &client_session_id)
                };

                let _ = gql.send_watch_gql(&store, payload).await;
                self.mark_pinged(&channel.request.channel_id, Instant::now()).await;

                // Claim channel points every 5 loops
                if loop_count % 5 == 0 {
                    if let Ok(pts_res) = gql.get_channel_points_context(&store, &channel.request.channel_login).await {
                        if let Some(claim_id) = pts_res.pointer("/data/channel/self/communityPoints/availableClaim/id").and_then(|v| v.as_str()) {
                            if let Some(ch_id) = pts_res.pointer("/data/channel/id").and_then(|v| v.as_str()) {
                                let _ = gql.claim_community_points(&store, ch_id, claim_id).await;
                            }
                        }
                    }
                }
            }

            // Emit engine-status-update
            {
                let inner = self.inner.lock().await;
                let active_logins: Vec<String> = inner.watched.iter().map(|w| w.request.channel_login.clone()).collect();
                let _ = app.emit("engine-status-update", active_logins);
            }

            loop_count += 1;
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }
}


use tauri::State;

pub struct EngineState {
    pub is_running: bool,
    pub cancel_token: Option<CancellationToken>,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct WatchedChannelStatus {
    pub channel_login: String,
    pub purpose: crate::models::WatchPurpose,
    pub uptime_secs: u64,
}

#[tauri::command]
pub async fn start_engine(
    app: AppHandle,
    state: State<'_, Arc<Mutex<EngineState>>>,
    engine: State<'_, Arc<ViewingEngine>>,
) -> Result<(), String> {
    let mut s = state.lock().await;
    if s.is_running {
        return Ok(());
    }

    s.is_running = true;
    let cancel_token = CancellationToken::new();
    s.cancel_token = Some(cancel_token.clone());

    let engine_clone = engine.inner().clone();
    let app_clone = app.clone();
    
    let store = match app.store("config.json") {
        Ok(s) => s,
        Err(e) => return Err(e.to_string()),
    };
    
    let gql = Arc::new(TwitchGQL::new());
    
    // We need user_id
    let user_id = if let Ok(ui) = gql.get_user_info(&store).await {
        crate::utils::json_id_to_string(ui.pointer("/data/currentUser/id")).unwrap_or_default()
    } else {
        String::new()
    };

    tokio::spawn(async move {
        engine_clone.run(gql, store, user_id, app_clone, cancel_token).await;
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_engine(
    state: State<'_, Arc<Mutex<EngineState>>>,
) -> Result<(), String> {
    let mut s = state.lock().await;
    s.is_running = false;
    if let Some(token) = s.cancel_token.take() {
        token.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_engine_status(
    engine: State<'_, Arc<ViewingEngine>>,
) -> Result<Vec<WatchedChannelStatus>, String> {
    let inner = engine.inner().inner.lock().await;
    let now = Instant::now();
    let status = inner.watched.iter().map(|w| WatchedChannelStatus {
        channel_login: w.request.channel_login.clone(),
        purpose: w.request.purpose.clone(),
        uptime_secs: now.duration_since(w.started_at).as_secs(),
    }).collect();
    Ok(status)
}

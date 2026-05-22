use crate::models::{
    AppConfig, BonusConfig, DailyStreaksConfig, PointsFarmingConfig, Priorities, Settings,
};
use serde_json::json;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const DEFAULT_CLIENT_ID: &str = "kd1unb4b3q4t58fwlpcbzcbnm76a8fp";

fn default_config() -> AppConfig {
    AppConfig {
        client_id: DEFAULT_CLIENT_ID.to_string(),
        priorities: Priorities {
            games: vec![],
            streamers: vec![],
        },
        bonus: BonusConfig {
            daily_streaks: DailyStreaksConfig {
                channels: vec![],
                auto_start: true,
                mode: "sequential".to_string(),
            },
            points_farming: PointsFarmingConfig {
                channels: vec![],
                auto_start: false,
                max_concurrent: 3,
            },
        },
        settings: Settings {
            language: "EN".to_string(),
            check_interval: 60,
            notifications_enabled: true,
            notify_active_drop: true,
            notify_claim: true,
            notify_campaign_finish: true,
            minimize_to_tray: true,
            show_tray_icon: true,
            auto_start_drops: false,
            mine_all_inventory: false,
        },
        schedule: Default::default(),
    }
}

fn normalize_unique(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if !trimmed.is_empty()
            && !normalized
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            normalized.push(trimmed.to_string());
        }
    }
    normalized
}

fn sanitize_config(mut config: AppConfig) -> AppConfig {
    config.client_id = DEFAULT_CLIENT_ID.to_string();
    config.priorities.games = normalize_unique(config.priorities.games);
    config.priorities.streamers = normalize_unique(config.priorities.streamers);
    config.bonus.daily_streaks.channels = normalize_unique(config.bonus.daily_streaks.channels);
    config.bonus.points_farming.channels = normalize_unique(config.bonus.points_farming.channels);
    if config.bonus.daily_streaks.mode != "parallel"
        && config.bonus.daily_streaks.mode != "sequential"
    {
        config.bonus.daily_streaks.mode = "sequential".to_string();
    }
    config.bonus.points_farming.max_concurrent =
        config.bonus.points_farming.max_concurrent.clamp(1, 10);
    config.settings.check_interval = config.settings.check_interval.clamp(10, 300);
    if !["EN", "FR", "ES", "DE"].contains(&config.settings.language.as_str()) {
        config.settings.language = "EN".to_string();
    }
    config
}

#[tauri::command]
pub async fn get_config(app: AppHandle) -> Result<AppConfig, String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;

    // Try to load existing config
    if let Some(config_json) = store.get("app_config") {
        log::info!("get_config: Found stored config: {:?}", config_json);
        match serde_json::from_value::<AppConfig>(config_json.clone()) {
            Ok(config) => {
                let sanitized = sanitize_config(config);
                log::info!(
                    "get_config: Parsed config successfully. Games: {:?}, Streamers: {:?}",
                    sanitized.priorities.games, sanitized.priorities.streamers
                );
                return Ok(sanitized);
            }
            Err(e) => {
                log::info!(
                    "get_config: Failed to parse config: {}. Raw: {:?}",
                    e, config_json
                );
            }
        }
    } else {
        log::info!("get_config: No stored config found, returning default");
    }

    // Return default config if none exists
    let default = default_config();
    Ok(default)
}

#[tauri::command]
pub async fn save_config(app: AppHandle, config: AppConfig) -> Result<(), String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    let config = sanitize_config(config);

    log::info!(
        "save_config: Saving config with games={:?}, streamers={:?}, streak_channels={:?}",
        config.priorities.games, config.priorities.streamers, config.bonus.daily_streaks.channels
    );

    store.set("app_config", json!(config));
    store
        .save()
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;

    log::info!("save_config: Config saved successfully");
    Ok(())
}

#[tauri::command]
pub async fn reset_config(app: AppHandle) -> Result<AppConfig, String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;

    let default = default_config();
    store.set("app_config", json!(default));
    store
        .save()
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;

    Ok(default)
}

#[tauri::command]
pub async fn search_games_cmd(
    app: AppHandle,
    query: String,
) -> Result<Vec<(String, String)>, String> {
    let gql = crate::api::TwitchGQL::new();
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;

    let res = gql.search_games(&store, &query).await?;

    let mut games = Vec::new();
    if let Some(games_data) = res.pointer("/data/searchFor/games/edges") {
        if let Some(edges_array) = games_data.as_array() {
            for edge in edges_array {
                if let Some(item) = edge.get("item") {
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = item
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .or_else(|| item.get("name").and_then(|v| v.as_str()))
                        .unwrap_or("")
                        .to_string();
                    if !id.is_empty() && !name.is_empty() {
                        games.push((id, name));
                    }
                }
            }
        }
    }

    Ok(games)
}

use crate::models::{AppConfig, Priorities, BonusConfig, DailyStreaksConfig, PointsFarmingConfig, Settings};
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
                channels: vec![
                    "shroud".to_string(),
                    "summit1g".to_string(),
                    "lirik".to_string(),
                    "pokimane".to_string(),
                    "ninja".to_string(),
                ],
                auto_start: true,
                mode: "parallel".to_string(),
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
        },
    }
}

#[tauri::command]
pub async fn get_config(app: AppHandle) -> Result<AppConfig, String> {
    let store = app.store("config.json").map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    
    // Try to load existing config
    if let Some(config_json) = store.get("app_config") {
        if let Ok(config) = serde_json::from_value::<AppConfig>(config_json) {
            // Update client_id if it changed
            if config.client_id != DEFAULT_CLIENT_ID {
                return Ok(default_config());
            }
            return Ok(config);
        }
    }
    
    // Return default config if none exists
    let default = default_config();
    Ok(default)
}

#[tauri::command]
pub async fn save_config(app: AppHandle, config: AppConfig) -> Result<(), String> {
    let store = app.store("config.json").map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    
    store.set("app_config", json!(config));
    store.save().map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
pub async fn reset_config(app: AppHandle) -> Result<AppConfig, String> {
    let store = app.store("config.json").map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    
    let default = default_config();
    store.set("app_config", json!(default));
    store.save().map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    
    Ok(default)
}

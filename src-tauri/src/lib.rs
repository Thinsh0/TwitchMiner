pub mod api;
pub mod auth;
pub mod miner;
pub mod streak_miner;
pub mod config;
pub mod models;

use tauri::{Manager, AppHandle};
use tauri_plugin_store::StoreExt;
use tokio::sync::Mutex;
use std::sync::Arc;
use crate::miner::MinerState;
use crate::streak_miner::StreakMinerState;

#[tauri::command]
async fn get_status(state: tauri::State<'_, Arc<Mutex<MinerState>>>) -> Result<models::MinerStatus, String> {
    let s = state.lock().await;
    Ok(s.get_status())
}

#[tauri::command]
async fn check_auth_status(app: AppHandle) -> Result<bool, String> {
    let store = app.store("config.json").map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    let token = store.get("access_token");
    let is_auth = token.is_some();
    
    if let Some(t) = token {
        let token_preview: String = t.as_str().unwrap_or_default().chars().take(20).collect();
        println!("Auth check - Token exists: {}...", token_preview);
    } else {
        println!("Auth check - No token found");
    }
    
    Ok(is_auth)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let miner_state = Arc::new(Mutex::new(MinerState::new()));
    let streak_miner_state = Arc::new(Mutex::new(StreakMinerState::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            app.manage(miner_state.clone());
            app.manage(streak_miner_state.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            check_auth_status,
            auth::start_auth,
            auth::poll_for_token,
            miner::start_miner,
            miner::stop_miner,
            miner::get_drop_campaigns,
            streak_miner::start_streak_miner,
            streak_miner::stop_streak_miner,
            streak_miner::get_streak_miner_status,
            config::get_config,
            config::save_config,
            config::reset_config,
            config::search_games_cmd
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
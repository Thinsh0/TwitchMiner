pub mod api;
pub mod auth;
pub mod miner;
pub mod models;

use tauri::Manager;
use tokio::sync::Mutex;
use std::sync::Arc;
use crate::miner::MinerState;

#[tauri::command]
async fn get_status(state: tauri::State<'_, Arc<Mutex<MinerState>>>) -> Result<models::MinerStatus, String> {
    let s = state.lock().await;
    Ok(s.get_status())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let miner_state = Arc::new(Mutex::new(MinerState::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            app.manage(miner_state.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            auth::start_auth,
            auth::poll_for_token,
            miner::start_miner,
            miner::stop_miner
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
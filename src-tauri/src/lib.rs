pub mod api;
pub mod auth;
pub mod config;
pub mod miner;
pub mod models;
pub mod streak_miner;

use crate::miner::MinerState;
use crate::streak_miner::StreakMinerState;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use tokio::sync::Mutex;

#[tauri::command]
async fn get_status(
    state: tauri::State<'_, Arc<Mutex<MinerState>>>,
) -> Result<models::MinerStatus, String> {
    let s = state.lock().await;
    Ok(s.get_status())
}

#[tauri::command]
async fn check_auth_status(app: AppHandle) -> Result<bool, String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    Ok(store
        .get("access_token")
        .and_then(|token| token.as_str().map(|value| !value.trim().is_empty()))
        .unwrap_or(false))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let miner_state = Arc::new(Mutex::new(MinerState::new()));
    let streak_miner_state = Arc::new(Mutex::new(StreakMinerState::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            app.manage(miner_state.clone());
            app.manage(streak_miner_state.clone());

            let quit_i = tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show_i = tauri::menu::MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let hide_i = tauri::menu::MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(app, &[&show_i, &hide_i, &quit_i])?;

            let _tray = tauri::tray::TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => std::process::exit(0),
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "hide" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            let args: Vec<String> = std::env::args().collect();
            if args.iter().any(|a| a == "--hidden") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let app = window.app_handle();
                let minimize = if let Ok(store) = app.store("config.json") {
                    store
                        .get("minimize_to_tray")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true)
                } else {
                    true
                };

                if minimize {
                    window.hide().unwrap();
                    api.prevent_close();
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            check_auth_status,
            auth::get_user_info,
            auth::start_auth,
            auth::poll_for_token,
            auth::logout,
            miner::start_miner,
            miner::stop_miner,
            miner::get_drop_campaigns,
            miner::get_claimed_drops,
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

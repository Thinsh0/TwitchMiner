use twitch_miner_core::{auth, config, miner, models, streak_miner};
use twitch_miner_core::miner::MinerState;
use twitch_miner_core::streak_miner::StreakMinerState;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use tokio::sync::Mutex;

mod wakelock;

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

#[cfg(target_os = "android")]
fn check_battery_optimization(app: &tauri::AppHandle) -> Result<bool, String> {
    let window = app.get_webview_window("main").ok_or("Failed to get main window")?;
    
    let (tx, rx) = std::sync::mpsc::channel();
    
    window.with_webview(move |webview| {
        webview.jni_handle().exec(move |env, context, _webview| {
            let res = (|| -> Result<bool, String> {
                // 1. Get package name
                let pkg_name_java = env.call_method(context, "getPackageName", "()Ljava/lang/String;", &[])
                    .map_err(|e| format!("Failed to getPackageName: {:?}", e))?
                    .l()
                    .map_err(|e| format!("Failed to get package name object: {:?}", e))?;
                    
                // 2. Get PowerManager
                let power_service = env.get_static_field("android/content/Context", "POWER_SERVICE", "Ljava/lang/String;")
                    .map_err(|e| format!("Failed to get POWER_SERVICE field: {:?}", e))?
                    .l()
                    .map_err(|e| format!("Failed to get POWER_SERVICE object: {:?}", e))?;
                    
                let pm = env.call_method(context, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[(&power_service).into()])
                    .map_err(|e| format!("Failed to getSystemService: {:?}", e))?
                    .l()
                    .map_err(|e| format!("Failed to get service object: {:?}", e))?;
                    
                // 3. Call isIgnoringBatteryOptimizations
                let is_ignoring = env.call_method(pm, "isIgnoringBatteryOptimizations", "(Ljava/lang/String;)Z", &[(&pkg_name_java).into()])
                    .map_err(|e| format!("Failed to call isIgnoringBatteryOptimizations: {:?}", e))?
                    .z()
                    .map_err(|e| format!("Failed to get boolean: {:?}", e))?;
                    
                Ok(is_ignoring)
            })();
            let _ = tx.send(res);
        });
    }).map_err(|e| format!("Webview error: {:?}", e))?;
    
    rx.recv().map_err(|e| format!("Failed to receive JNI result: {:?}", e))?
}

#[cfg(target_os = "android")]
fn request_battery_exemption(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("Failed to get main window")?;
    
    let (tx, rx) = std::sync::mpsc::channel();
    
    window.with_webview(move |webview| {
        webview.jni_handle().exec(move |env, context, _webview| {
            let res = (|| -> Result<(), String> {
                // 1. Get ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS
                let action_str = env.get_static_field("android/provider/Settings", "ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS", "Ljava/lang/String;")
                    .map_err(|e| format!("Failed to get ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS: {:?}", e))?
                    .l()
                    .map_err(|e| format!("Failed to get action string object: {:?}", e))?;
                    
                // 2. Create new Intent
                let intent_class = env.find_class("android/content/Intent")
                    .map_err(|e| format!("Failed to find Intent class: {:?}", e))?;
                    
                let intent = env.new_object(intent_class, "(Ljava/lang/String;)V", &[(&action_str).into()])
                    .map_err(|e| format!("Failed to create Intent object: {:?}", e))?;
                    
                // 3. Call context.startActivity(intent)
                env.call_method(context, "startActivity", "(Landroid/content/Intent;)V", &[(&intent).into()])
                    .map_err(|e| format!("Failed to call startActivity: {:?}", e))?;
                    
                Ok(())
            })();
            let _ = tx.send(res);
        });
    }).map_err(|e| format!("Webview error: {:?}", e))?;
    
    rx.recv().map_err(|e| format!("Failed to receive JNI result: {:?}", e))?
}

#[tauri::command]
async fn check_battery_optimizations(_app: AppHandle) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        check_battery_optimization(&_app)
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(true)
    }
}

#[tauri::command]
async fn request_battery_optimization(_app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        request_battery_exemption(&_app)
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(())
    }
}

#[tauri::command]
async fn update_foreground_label(app: AppHandle, label: String) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_background_service::mobile::MobileLifecycle;
        if let Some(lifecycle) = app.try_state::<Arc<MobileLifecycle<tauri::Wry>>>() {
            log::info!("Updating foreground service notification label to: {}", label);
            lifecycle
                .start_keepalive(
                    &label,
                    "dataSync",
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .map(|_| ())
                .map_err(|e| {
                    log::error!("Failed to update keepalive label: {:?}", e);
                    e.to_string()
                })?;
        } else {
            log::warn!("MobileLifecycle state not found in AppHandle");
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        let _ = label;
    }
    Ok(())
}

pub struct TwitchMinerService;

#[async_trait::async_trait]
impl tauri_plugin_background_service::BackgroundService<tauri::Wry> for TwitchMinerService {
    async fn init(&mut self, _ctx: &tauri_plugin_background_service::ServiceContext<tauri::Wry>) -> Result<(), tauri_plugin_background_service::ServiceError> {
        log::info!("TwitchMinerService background init completed.");
        Ok(())
    }

    async fn run(&mut self, ctx: &tauri_plugin_background_service::ServiceContext<tauri::Wry>) -> Result<(), tauri_plugin_background_service::ServiceError> {
        log::info!("TwitchMinerService background run loop started.");
        let _ = crate::wakelock::acquire_wakelock();
        
        let app = ctx.app.clone();
        
        // Retrieve our managed states defensively
        let miner_state = match app.try_state::<Arc<Mutex<MinerState>>>() {
            Some(state) => state,
            None => {
                log::error!("MinerState is not managed yet, aborting background service.");
                return Ok(());
            }
        };
        let streak_miner_state = match app.try_state::<Arc<Mutex<StreakMinerState>>>() {
            Some(state) => state,
            None => {
                log::error!("StreakMinerState is not managed yet, aborting background service.");
                return Ok(());
            }
        };
        
        // Auto-refresh token and start background mining if authenticated
        let is_authed = match check_auth_status(app.clone()).await {
            Ok(authed) => authed,
            Err(_) => false,
        };
        
        if is_authed {
            log::info!("User is authenticated, starting background miners.");
            
            // Check if we need to refresh the token first
            if let Ok(store) = app.store("config.json") {
                if let Some(token) = store.get("refresh_token").and_then(|v| v.as_str().map(|s| s.to_string())) {
                    if !token.trim().is_empty() {
                        log::info!("Attempting automatic token refresh at background startup.");
                        let _ = auth::refresh_access_token(&store).await;
                    }
                }
            }
            if let Ok(store) = app.store("config.json") {
                let app_config: twitch_miner_core::models::AppConfig = store
                    .get("app_config")
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or_default();
                    
                if app_config.settings.auto_start_drops {
                    log::info!("Auto-starting drops miner in background...");
                    let _ = miner::start_miner(app.clone(), miner_state.clone(), Some(true)).await;
                } else {
                    log::info!("Auto-start drops is disabled, not starting drops miner.");
                }
                
                if app_config.bonus.daily_streaks.auto_start || app_config.bonus.points_farming.auto_start {
                    log::info!("Auto-starting streak miner in background...");
                    let _ = streak_miner::start_streak_miner(app.clone(), streak_miner_state.clone(), Some(true)).await;
                } else {
                    log::info!("Auto-start streak is disabled, not starting streak miner.");
                }
            } else {
                log::info!("Failed to load config, falling back to unconditional start with abort checks.");
                let _ = miner::start_miner(app.clone(), miner_state.clone(), Some(true)).await;
                let _ = streak_miner::start_streak_miner(app.clone(), streak_miner_state.clone(), Some(true)).await;
            }
        } else {
            log::info!("User is not authenticated, background miners did not start.");
        }

        // Keep running until background service is cancelled or shut down
        tokio::select! {
            _ = ctx.shutdown.cancelled() => {
                log::info!("TwitchMinerService background service cancelled.");
                let _ = crate::wakelock::release_wakelock();
            }
        }
        
        Ok(())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let miner_state = Arc::new(Mutex::new(MinerState::new()));
    let streak_miner_state = Arc::new(Mutex::new(StreakMinerState::new()));

    tauri::Builder::default()
        .manage(miner_state)
        .manage(streak_miner_state)
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_background_service::init_with_service(|| TwitchMinerService))
        .invoke_handler(tauri::generate_handler![
            get_status,
            check_auth_status,
            check_battery_optimizations,
            request_battery_optimization,
            update_foreground_label,
            auth::get_user_info,
            auth::start_auth,
            auth::poll_for_token,
            auth::logout,
            miner::start_miner,
            miner::stop_miner,
            miner::get_drop_campaigns,
            miner::get_claimed_drops,
            miner::claim_drop_cmd,
            streak_miner::start_streak_miner,
            streak_miner::stop_streak_miner,
            streak_miner::get_streak_miner_status,
            config::get_config,
            config::save_config,
            config::reset_config,
            config::search_games_cmd,
            config::ping
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}


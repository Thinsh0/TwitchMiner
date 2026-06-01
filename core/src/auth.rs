use crate::models::AuthInitResponse;
use reqwest::Client;
use serde_json::json;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

static TOKEN_REFRESH_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const CLIENT_ID: &str = "kd1unb4b3q4t58fwlpcbzcbnm76a8fp"; // Twitch Android App Client ID
const SCOPES: &str = "openid user:read:email";

#[tauri::command]
pub async fn start_auth() -> Result<AuthInitResponse, String> {
    let client = Client::new();

    let res = client
        .post("https://id.twitch.tv/oauth2/device")
        .form(&[("client_id", CLIENT_ID), ("scopes", SCOPES)])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        let data: AuthInitResponse = res.json().await.map_err(|e| e.to_string())?;
        Ok(data)
    } else {
        Err(format!("Auth failed: {}", res.status()))
    }
}

#[tauri::command]
pub async fn poll_for_token(
    app: AppHandle,
    device_code: String,
    interval: u64,
) -> Result<String, String> {
    let client = Client::new();
    let mut current_interval = interval;

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(current_interval)).await;

        let res = client
            .post("https://id.twitch.tv/oauth2/token")
            .form(&[
                ("client_id", CLIENT_ID),
                ("scopes", SCOPES),
                ("device_code", &device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = res.status();
        let text = res.text().await.unwrap_or_default();

        if status.is_success() {
            // Parse as Value for maximum flexibility
            let token_data: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| format!("Failed to parse auth response JSON: {}", e))?;

            // Extract required fields
            let access_token = token_data
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or("Missing access_token in response")?;

            let refresh_token = token_data
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .ok_or("Missing refresh_token in response")?;
                
            let expires_in = token_data
                .get("expires_in")
                .and_then(|v| v.as_u64())
                .unwrap_or(14400);

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let expires_at = now + expires_in.saturating_sub(300);

            log::info!("Access token length: {}", access_token.len());
            log::info!("Refresh token length: {}", refresh_token.len());

            // Save to store
            let store = app
                .store("config.json")
                .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
            log::info!("Store created successfully");

            store.set("access_token", json!(access_token));
            store.set("refresh_token", json!(refresh_token));
            store.set("expires_at", json!(expires_at));
            log::info!("Token set in store");

            store
                .save()
                .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
            log::info!("Store saved successfully");

            return Ok("Authenticated successfully!".to_string());
        } else if status == 400 {
            if let Ok(error_data) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(msg) = error_data.get("message").and_then(|m| m.as_str()) {
                    match msg {
                        "authorization_pending" => continue,
                        "slow_down" => {
                            current_interval += 2;
                            continue;
                        }
                        "expired_token" => {
                            return Err("Code expired. Please try again.".to_string())
                        }
                        _ => return Err(format!("Auth Error: {}", msg)),
                    }
                }
            }
        }

        return Err(format!("Unexpected response: {}", text));
    }
}

#[tauri::command]
pub async fn logout(app: AppHandle) -> Result<(), String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    store.delete("access_token");
    store.delete("refresh_token");
    store.delete("expires_at");
    store.delete("id_token");
    store
        .save()
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    Ok(())
}

pub async fn refresh_access_token(
    store: &std::sync::Arc<tauri_plugin_store::Store<tauri::Wry>>,
) -> Result<(), String> {
    let _lock = TOKEN_REFRESH_MUTEX.lock().await;

    // Fast path: maybe another thread already refreshed it while we were waiting for the lock
    if let Some(expires_at) = store.get("expires_at").and_then(|v| v.as_u64()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now < expires_at {
            return Ok(());
        }
    }

    let refresh_token = store
        .get("refresh_token")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .ok_or("No refresh token available")?;

    let client = Client::new();
    let res = client
        .post("https://id.twitch.tv/oauth2/token")
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        let token_data: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;

        let new_access_token = token_data
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or("Missing access_token in response")?;

        let new_refresh_token = token_data
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .ok_or("Missing refresh_token in response")?;

        let expires_in = token_data
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(14400);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expires_at = now + expires_in.saturating_sub(300);

        store.set("access_token", json!(new_access_token));
        store.set("refresh_token", json!(new_refresh_token));
        store.set("expires_at", json!(expires_at));
        store.save().map_err(|e| e.to_string())?;

        Ok(())
    } else {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if status.as_u16() == 400 || status.as_u16() == 401 {
            // Only drop the session if Twitch explicitly tells us the grant is invalid
            if text.contains("invalid_grant") || text.contains("invalid_client") {
                log::warn!("Fatal token refresh error, dropping session: {}", text);
                store.delete("access_token");
                store.delete("refresh_token");
                store.delete("expires_at");
                let _ = store.save();
            } else {
                log::warn!("Non-fatal token refresh error ({}): {}", status, text);
            }
        } else {
            log::warn!("Failed to refresh token ({}): {}", status, text);
        }
        Err(format!("Failed to refresh token ({}): {}", status, text))
    }
}

pub async fn ensure_valid_token(
    store: &std::sync::Arc<tauri_plugin_store::Store<tauri::Wry>>,
) -> Result<(), String> {
    if let Some(expires_at) = store.get("expires_at").and_then(|v| v.as_u64()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now < expires_at {
            // Token is still valid
            return Ok(());
        }
    }
    
    // Token is expired or expires_at is missing, refresh it
    refresh_access_token(store).await
}

pub async fn validate_access_token(
    store: &std::sync::Arc<tauri_plugin_store::Store<tauri::Wry>>,
) -> Result<bool, String> {
    let access_token = match store.get("access_token") {
        Some(v) => match v.as_str() {
            Some(s) => s.to_string(),
            None => return Ok(false),
        },
        None => return Ok(false),
    };

    let client = Client::new();
    let res = client
        .get("https://id.twitch.tv/oauth2/validate")
        .header("Authorization", format!("OAuth {}", access_token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        Ok(true)
    } else {
        log::info!("Token validation failed with status {}", res.status());
        Ok(false)
    }
}

#[tauri::command]
pub async fn get_user_info(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let store = app
        .store("config.json")
        .map_err(|e: tauri_plugin_store::Error| e.to_string())?;
    let store_arc = std::sync::Arc::new(store);

    let gql = crate::api::TwitchGQL::new();
    gql.get_user_info(&store_arc).await
}

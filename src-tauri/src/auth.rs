use reqwest::Client;
use serde_json::json;
use crate::models::{AuthInitResponse, TokenResponse};
use tauri_plugin_store::StoreExt;
use tauri::AppHandle;

const CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko"; // Typical Switch/Web Client ID
const SCOPES: &str = "openid user:read:email";

#[tauri::command]
pub async fn start_auth() -> Result<AuthInitResponse, String> {
    let client = Client::new();
    
    let res = client.post("https://id.twitch.tv/oauth2/device")
        .form(&[
            ("client_id", CLIENT_ID),
            ("scopes", SCOPES)
        ])
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
pub async fn poll_for_token(app: AppHandle, device_code: String, interval: u64) -> Result<String, String> {
    let client = Client::new();
    let mut current_interval = interval;

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(current_interval)).await;

        let res = client.post("https://id.twitch.tv/oauth2/token")
            .form(&[
                ("client_id", CLIENT_ID),
                ("scopes", SCOPES),
                ("device_code", &device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")
            ])
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = res.status();
        let text = res.text().await.unwrap_or_default();

        if status.is_success() {
            if let Ok(token_data) = serde_json::from_str::<TokenResponse>(&text) {
                // Save to store
                let store = app.store("config.json").map_err(|e| e.to_string())?;
                store.set("access_token", json!(token_data.access_token));
                store.set("refresh_token", json!(token_data.refresh_token));
                store.save().map_err(|e| e.to_string())?;
                
                return Ok("Authenticated successfully!".to_string());
            }
        } else if status == 400 {
            if let Ok(error_data) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(msg) = error_data.get("message").and_then(|m| m.as_str()) {
                    match msg {
                        "authorization_pending" => continue,
                        "slow_down" => {
                            current_interval += 2;
                            continue;
                        },
                        "expired_token" => return Err("Code expired. Please try again.".to_string()),
                        _ => return Err(format!("Auth Error: {}", msg))
                    }
                }
            }
        }
        
        return Err(format!("Unexpected response: {}", text));
    }
}
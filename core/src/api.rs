use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use flate2::write::GzEncoder;
use flate2::Compression;
use regex::Regex;
use reqwest::{header, Client};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::Arc;
use tauri_plugin_store::Store;
use uuid::Uuid;

pub struct TwitchGQL {
    client: Client,
    client_id: String,
    device_id: String,
}

impl TwitchGQL {
    pub fn new() -> Self {
        let device_id = format!(
            "ecd{}",
            Uuid::new_v4().to_string().replace("-", "")[..29].to_string()
        );

        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_default();

        Self {
            client,
            client_id: "kimne78kx3ncx6brgo4mv6wki5h1ko".to_string(),
            device_id,
        }
    }

    fn get_auth_token(store: &Arc<Store<tauri::Wry>>) -> Option<String> {
        store
            .get("access_token")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    }

    fn build_headers(&self, token: &Option<String>) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert("Client-ID", self.client_id.parse().unwrap());
        headers.insert("X-Device-Id", self.device_id.parse().unwrap());
        headers.insert("Content-Type", "application/json".parse().unwrap());
        headers.insert("Origin", "https://www.twitch.tv".parse().unwrap());
        headers.insert("Referer", "https://www.twitch.tv/".parse().unwrap());
        headers.insert("Accept", "*/*".parse().unwrap());
        headers.insert("Accept-Language", "en-US".parse().unwrap());
        headers.insert("Pragma", "no-cache".parse().unwrap());
        headers.insert("Cache-Control", "no-cache".parse().unwrap());

        if let Some(t) = token {
            if let Ok(auth_header) = format!("OAuth {}", t).parse() {
                headers.insert("Authorization", auth_header);
            }
        }

        headers
    }

    pub async fn post(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        operation_name: &str,
        query: &str,
        variables: Value,
    ) -> Result<Value, String> {
        let payload = json!({
            "operationName": operation_name,
            "query": query,
            "variables": variables
        });

        let mut last_error = String::new();
        for attempt in 1..=3 {
            let token = Self::get_auth_token(store);
            let res = match self
                .client
                .post("https://gql.twitch.tv/gql")
                .headers(self.build_headers(&token))
                .json(&payload)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_error = e.to_string();
                    let delay = (1000 * attempt) + rand::random::<u64>() % 500;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    continue;
                }
            };

            if res.status() == reqwest::StatusCode::UNAUTHORIZED {
                log::info!("Got 401 on GQL post, attempting token refresh...");
                if let Ok(()) = crate::auth::refresh_access_token(store).await {
                    continue;
                }
            }

            if res.status().is_server_error()
                || res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
            {
                last_error = format!("HTTP {}", res.status());
                let delay = (1000 * attempt) + rand::random::<u64>() % 500;
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                continue;
            }

            let json_val: Value = match res.json().await {
                Ok(v) => v,
                Err(e) => {
                    last_error = e.to_string();
                    let delay = (1000 * attempt) + rand::random::<u64>() % 500;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    continue;
                }
            };

            if let Some(errors) = json_val.get("errors").and_then(|e| e.as_array()) {
                let is_unauth = errors.iter().any(|e| {
                    e.get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("")
                        .contains("unauthenticated")
                        || e.get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("")
                            .contains("failed to verify")
                });

                if is_unauth {
                    log::info!("Got GQL unauthenticated error, attempting token refresh...");
                    if let Ok(()) = crate::auth::refresh_access_token(store).await {
                        continue;
                    }
                }
            }

            return Ok(json_val);
        }

        Err(format!(
            "GQL post failed after 3 attempts. Last error: {}",
            last_error
        ))
    }

    pub async fn post_persisted(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        operation_name: &str,
        sha256: &str,
        variables: Value,
    ) -> Result<Value, String> {
        let payload = json!({
            "operationName": operation_name,
            "extensions": {
                "persistedQuery": {
                    "version": 1,
                    "sha256Hash": sha256
                }
            },
            "variables": variables
        });

        let mut last_error = String::new();
        for attempt in 1..=3 {
            let token = Self::get_auth_token(store);
            let res = match self
                .client
                .post("https://gql.twitch.tv/gql")
                .headers(self.build_headers(&token))
                .json(&payload)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_error = e.to_string();
                    let delay = (1000 * attempt) + rand::random::<u64>() % 500;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    continue;
                }
            };

            if res.status() == reqwest::StatusCode::UNAUTHORIZED {
                log::info!("Got 401 on GQL post_persisted, attempting token refresh...");
                if let Ok(()) = crate::auth::refresh_access_token(store).await {
                    continue;
                }
            }

            if res.status().is_server_error()
                || res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
            {
                last_error = format!("HTTP {}", res.status());
                let delay = (1000 * attempt) + rand::random::<u64>() % 500;
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                continue;
            }

            let json_val: Value = match res.json().await {
                Ok(v) => v,
                Err(e) => {
                    last_error = e.to_string();
                    let delay = (1000 * attempt) + rand::random::<u64>() % 500;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    continue;
                }
            };

            if let Some(errors) = json_val.get("errors").and_then(|e| e.as_array()) {
                // Detect persisted query hash mismatch — treat as hard error
                let is_not_found = errors.iter().any(|e| {
                    e.get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("")
                        .contains("PersistedQueryNotFound")
                });
                if is_not_found {
                    return Err(format!(
                        "PersistedQueryNotFound for operation '{}'. The GQL hash may be stale.",
                        operation_name
                    ));
                }

                let is_unauth = errors.iter().any(|e| {
                    e.get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("")
                        .contains("unauthenticated")
                        || e.get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("")
                            .contains("failed to verify")
                });

                if is_unauth {
                    log::warn!(
                        "Got GQL unauthenticated error (persisted '{}'), attempting token refresh...",
                        operation_name
                    );
                    if let Ok(()) = crate::auth::refresh_access_token(store).await {
                        continue;
                    }
                    // Refresh also failed — surface the error clearly
                    return Err(format!(
                        "Authentication failed for '{}' and token refresh did not recover.",
                        operation_name
                    ));
                }
            }

            return Ok(json_val);
        }

        Err(format!(
            "GQL post_persisted failed after 3 attempts. Last error: {}",
            last_error
        ))
    }

    pub async fn get_inventory(&self, store: &Arc<Store<tauri::Wry>>) -> Result<Value, String> {
        let sha256 = "d86775d0ef16a63a33ad52e80eaff963b2d5b72fada7c991504a57496e1d8e4b";
        let vars = json!({ "fetchRewardCampaigns": false });
        self.post_persisted(store, "Inventory", sha256, vars).await
    }

    pub async fn get_all_campaigns(&self, store: &Arc<Store<tauri::Wry>>) -> Result<Value, String> {
        let sha256 = "5a4da2ab3d5b47c9f9ce864e727b2cb346af1e3ea8b897fe8f704a97ff017619";
        let vars = json!({ "fetchRewardCampaigns": false });
        self.post_persisted(store, "ViewerDropsDashboard", sha256, vars).await
    }

    pub async fn get_user_info(&self, store: &Arc<Store<tauri::Wry>>) -> Result<Value, String> {
        let query =
            "query UserInfo { currentUser { id login displayName profileImageURL(width: 50) } }";
        self.post(store, "UserInfo", query, json!({})).await
    }

    pub async fn claim_drop(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        drop_id: &str,
    ) -> Result<Value, String> {
        // Use a raw GQL mutation instead of a persisted query to avoid
        // PersistedQueryNotFound errors caused by Twitch rotating sha256 hashes.
        let query = "mutation DropsPage_ClaimDropRewards($input: ClaimDropRewardsInput!) {
            claimDropRewards(input: $input) {
                isUserAccountConnected
                status
            }
        }";
        let vars = json!({
            "input": {
                "dropInstanceID": drop_id
            }
        });
        let result = self.post(store, "DropsPage_ClaimDropRewards", query, vars).await;
        
        match result {
            Ok(val) => {
                log::info!("claim_drop response for {}: {:?}", drop_id, val);
                if let Some(errors) = val.get("errors") {
                    return Err(format!("GraphQL errors in response: {}", errors));
                }
                if let Some(status) = val.pointer("/data/claimDropRewards/status").and_then(|s| s.as_str()) {
                    if status != "SUCCESS" && status != "ELIGIBLE_FOR_ALL" && status != "DROP_INSTANCE_ALREADY_CLAIMED" {
                        return Err(format!("Claim failed with status: {}", status));
                    }
                }
                Ok(val)
            }
            Err(e) => {
                log::error!("claim_drop error for {}: {}", drop_id, e);
                Err(e)
            }
        }
    }

    pub async fn get_live_streams(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        game_id: &str,
    ) -> Result<Value, String> {
        let query = "
        query GetLiveStreams($gameID: ID!) {
            game(id: $gameID) {
                streams(first: 20, options: {sort: VIEWER_COUNT}) {
                    edges {
                        node {
                            id
                            broadcaster {
                                id
                                login
                                displayName
                                profileImageURL(width: 70)
                            }
                            viewersCount
                            tags {
                                id
                                localizedName
                            }
                            freeformTags {
                                name
                            }
                        }
                    }
                }
            }
        }";
        let vars = json!({ "gameID": game_id });
        self.post(store, "GetLiveStreams", query, vars).await
    }

    pub async fn get_available_drops(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        channel_id: &str,
    ) -> Result<Value, String> {
        let sha256 = "9a62a09bce5b53e26e64a671e530bc599cb6aab1e5ba3cbd5d85966d3940716f";
        let vars = json!({ "channelID": channel_id });
        self.post_persisted(store, "DropsHighlightService_AvailableDrops", sha256, vars)
            .await
    }

    pub async fn get_stream_info(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        channel_login: &str,
    ) -> Result<Value, String> {
        let query = "
        query GetStreamInfo($login: String!) {
            user(login: $login) {
                id
                profileImageURL(width: 70)
                stream {
                    id
                    game {
                        id
                        name
                    }
                    viewersCount
                }
            }
        }";
        let vars = json!({ "login": channel_login });
        self.post(store, "GetStreamInfo", query, vars).await
    }

    pub async fn get_playback_access_token(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        channel_login: &str,
    ) -> Result<Value, String> {
        let query = "
        query PlaybackAccessToken($login: String!, $template: String) {
            streamPlaybackAccessToken(channelName: $login, params: {platform: \"web\", playerBackend: \"mediaplayer\", playerType: \"site\"}) {
                value
                signature
            }
        }";
        let vars = json!({ "login": channel_login, "template": serde_json::Value::Null });
        self.post(store, "PlaybackAccessToken", query, vars).await
    }

    pub fn get_slug(&self, game_name: &str) -> String {
        let mut slug = game_name.to_lowercase();
        slug = slug.replace('\'', "");
        let re = Regex::new(r"\W+").unwrap();
        slug = re.replace_all(&slug, "-").to_string();
        let re_dup = Regex::new(r"-{2,}").unwrap();
        slug = re_dup.replace_all(&slug, "-").to_string();
        slug.trim_matches('-').to_string()
    }

    pub async fn search_games(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        query_text: &str,
    ) -> Result<Value, String> {
        let query = "
        query SearchFor($query: String!) {
            searchFor(userQuery: $query, platform: \"web\", target: {index: GAME}) {
                games {
                    edges {
                        item {
                            ... on Game {
                                id
                                displayName
                                name
                                boxArtURL(width: 52, height: 72)
                            }
                        }
                    }
                }
            }
        }";
        let vars = json!({ "query": query_text });
        self.post(store, "SearchFor", query, vars).await
    }

    pub async fn send_watch_gql(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        payload_data: Value,
    ) -> Result<bool, String> {
        let json_str = serde_json::to_string(&payload_data).map_err(|e| e.to_string())?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(json_str.as_bytes()).map_err(|e| e.to_string())?;
        let compressed_data = encoder.finish().map_err(|e| e.to_string())?;

        let b64_data = STANDARD.encode(compressed_data);

        let query = "
mutation SendEvents($input: SendSpadeEventsInput!) {
    sendSpadeEvents(input: $input) {
        statusCode
    }
}
";
        let vars = json!({
            "input": {
                "data": b64_data,
                "repository": "twilight",
                "encoding": "GZIP_B64"
            }
        });

        match self.post(store, "SendEvents", query, vars).await {
            Ok(res) => {
                if let Some(status) = res.pointer("/data/sendSpadeEvents/statusCode") {
                    if status.as_i64() == Some(204) {
                        log::info!("send_watch_gql: OK (204)");
                        return Ok(true);
                    }
                }
                log::info!("send_watch_gql: Unexpected response {:?}", res);
                Ok(false)
            }
            Err(e) => {
                log::info!("send_watch_gql: FAILED with error {}", e);
                Err(e)
            }
        }
    }

    /// Fetch real-time drop session progress for the current channel.
    /// Returns the GQL response containing `data.currentUser.dropCurrentSession`.
    pub async fn get_drop_current_session(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        channel_id: &str,
    ) -> Result<Value, String> {
        let sha256 = "4d06b702d25d652afb9ef835d2a550031f1cf762b193523a92166f40ea3d142b";
        let vars = json!({ 
            "channelID": channel_id,
            "channelLogin": ""
        });
        self.post_persisted(store, "DropCurrentSessionContext", sha256, vars).await
    }

    pub async fn get_channel_points_context(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        channel_login: &str,
    ) -> Result<Value, String> {
        let query = "
        query GetChannelPointsContext($login: String!) {
            channel(name: $login) {
                id
                displayName
                profileImageURL(width: 50)
                self {
                    communityPoints {
                        balance
                        availableClaim {
                            id
                        }
                    }
                }
            }
        }";
        let vars = json!({ "login": channel_login });
        self.post(store, "GetChannelPointsContext", query, vars)
            .await
    }

    pub async fn claim_community_points(
        &self,
        store: &Arc<Store<tauri::Wry>>,
        channel_id: &str,
        claim_id: &str,
    ) -> Result<Value, String> {
        let query = "
        mutation ClaimCommunityPoints($input: ClaimCommunityPointsInput!) {
            claimCommunityPoints(input: $input) {
                pointsEarned
                claim {
                    id
                    reason
                }
            }
        }";
        let vars = json!({ "input": { "channelID": channel_id, "claimID": claim_id } });
        self.post(store, "ClaimCommunityPoints", query, vars).await
    }
}

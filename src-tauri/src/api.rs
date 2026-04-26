use reqwest::{Client, header};
use serde_json::{json, Value};
use uuid::Uuid;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use tauri_plugin_store::Store;
use std::sync::Arc;
use regex::Regex;

pub struct TwitchGQL {
    client: Client,
    client_id: String,
    device_id: String,
}

impl TwitchGQL {
    pub fn new() -> Self {
        let device_id = format!("ecd{}", Uuid::new_v4().to_string().replace("-", "")[..29].to_string());
        
        let client = Client::builder()
            .user_agent("Dalvik/2.1.0 (Linux; U; Android 16; SM-S911B Build/TP1A.220624.014) tv.twitch.android.app/25.3.0/2503006")
            .build()
            .unwrap_or_default();

        Self {
            client,
            client_id: "kimne78kx3ncx6brgo4mv6wki5h1ko".to_string(),
            device_id,
        }
    }

    fn get_auth_token(store: &Arc<Store<tauri::Wry>>) -> Option<String> {
        store.get("access_token").and_then(|v| v.as_str().map(|s| s.to_string()))
    }

    fn build_headers(&self, token: &Option<String>) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert("Client-ID", self.client_id.parse().unwrap());
        headers.insert("X-Device-Id", self.device_id.parse().unwrap());
        headers.insert("Content-Type", "application/json".parse().unwrap());
        
        if let Some(t) = token {
            if let Ok(auth_header) = format!("OAuth {}", t).parse() {
                headers.insert("Authorization", auth_header);
            }
        }
        
        headers
    }

    pub async fn post(&self, store: &Arc<Store<tauri::Wry>>, operation_name: &str, query: &str, variables: Value) -> Result<Value, String> {
        let token = Self::get_auth_token(store);
        let payload = json!({
            "operationName": operation_name,
            "query": query,
            "variables": variables
        });

        let res = self.client.post("https://gql.twitch.tv/gql")
            .headers(self.build_headers(&token))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        res.json().await.map_err(|e| e.to_string())
    }

    pub async fn post_persisted(&self, store: &Arc<Store<tauri::Wry>>, operation_name: &str, sha256: &str, variables: Value) -> Result<Value, String> {
        let token = Self::get_auth_token(store);
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

        let res = self.client.post("https://gql.twitch.tv/gql")
            .headers(self.build_headers(&token))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        res.json().await.map_err(|e| e.to_string())
    }

    pub async fn get_inventory(&self, store: &Arc<Store<tauri::Wry>>) -> Result<Value, String> {
        let sha256 = "d86775d0ef16a63a33ad52e80eaff963b2d5b72fada7c991504a57496e1d8e4b";
        let vars = json!({ "fetchRewardCampaigns": false });
        self.post_persisted(store, "Inventory", sha256, vars).await
    }

    pub async fn get_user_info(&self, store: &Arc<Store<tauri::Wry>>) -> Result<Value, String> {
        let query = "query UserInfo { currentUser { id login displayName profileImageURL(width: 50) } }";
        self.post(store, "UserInfo", query, json!({})).await
    }

    pub async fn claim_drop(&self, store: &Arc<Store<tauri::Wry>>, drop_id: &str) -> Result<Value, String> {
        let sha256 = "a455deea71bdc9015b78eb49f4acfbce8baa7ccbedd28e549bb025bd0f751930";
        let vars = json!({
            "input": {
                "dropInstanceID": drop_id
            }
        });
        self.post_persisted(store, "DropsPage_ClaimDropRewards", sha256, vars).await
    }

    pub async fn get_live_streams(&self, store: &Arc<Store<tauri::Wry>>, game_id: &str) -> Result<Value, String> {
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

    pub async fn get_available_drops(&self, store: &Arc<Store<tauri::Wry>>, channel_id: &str) -> Result<Value, String> {
        let sha256 = "9a62a09bce5b53e26e64a671e530bc599cb6aab1e5ba3cbd5d85966d3940716f";
        let vars = json!({ "channelID": channel_id });
        self.post_persisted(store, "DropsHighlightService_AvailableDrops", sha256, vars).await
    }

    pub async fn get_stream_info(&self, store: &Arc<Store<tauri::Wry>>, channel_login: &str) -> Result<Value, String> {
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

    pub async fn get_playback_access_token(&self, store: &Arc<Store<tauri::Wry>>, channel_login: &str) -> Result<Value, String> {
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

    pub async fn search_games(&self, store: &Arc<Store<tauri::Wry>>, query_text: &str) -> Result<Value, String> {
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

    pub async fn get_spade_url(&self, streamer_login: &str) -> Result<String, String> {
        // Retry logic with exponential backoff
        for attempt in 1..=3 {
            match self._get_spade_url_internal(streamer_login).await {
                Ok(url) => return Ok(url),
                Err(e) => {
                    if attempt == 3 {
                        return Err(format!("Failed after 3 attempts: {}", e));
                    }
                    // Wait with jitter before retry
                    let delay = (1000 * attempt) + rand::random::<u64>() % 500;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
            }
        }
        Err("Spade URL not found".to_string())
    }

    async fn _get_spade_url_internal(&self, streamer_login: &str) -> Result<String, String> {
        let url = format!("https://www.twitch.tv/{}", streamer_login);
        println!("get_spade_url: Fetching {}", url);
        
        // Use a full desktop browser User-Agent — Twitch serves different HTML for mobile
        let desktop_ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
        
        let res = self.client.get(&url)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.5")
            .header("User-Agent", desktop_ua)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch page: {}", e))?;

        println!("get_spade_url: HTTP status {}", res.status());
        
        if !res.status().is_success() {
            return Err(format!("HTTP {}: Failed to fetch streamer page", res.status()));
        }

        let text = res.text().await.map_err(|e| format!("Failed to read response: {}", e))?;
        println!("get_spade_url: Got {} bytes of HTML", text.len());

        // Patterns ordered from broadest (TwitchDropsMiner style) to most specific
        let patterns: Vec<&str> = vec![
            // TDM-style broad pattern — matches current Twitch format
            r#""spade_?url": ?"(https://[.\w\-/]+)""#,
            // Legacy beacon patterns
            r#""beacon_?url": ?"(https://[.\w\-/]+)""#,
            r#""beacon_url": ?"(https://video-edge-[.\w\-/]+\.ts\?[^"]*)"#,
            r#""beacon-url": ?"(https://video-edge-[.\w\-/]+\.ts\?[^"]*)"#,
        ];

        // Try main page first
        if let Some(found) = Self::try_extract_spade(&text, &patterns) {
            println!("get_spade_url: Found spade URL in main page: {}", &found[..found.len().min(80)]);
            return Ok(found);
        }
        println!("get_spade_url: No spade URL in main page, trying settings.js...");
        
        // Fallback: extract from settings.js
        let re_settings = Regex::new(r#"src="(https://[\w.]+/config/settings\.[0-9a-f]{32}\.js)""#)
            .map_err(|e| format!("Settings regex error: {}", e))?;
            
        if let Some(caps) = re_settings.captures(&text) {
            let settings_url = &caps[1];
            println!("get_spade_url: Found settings.js: {}", settings_url);
            match self.client.get(settings_url)
                .header("User-Agent", desktop_ua)
                .send().await
            {
                Ok(res_js) => {
                    if res_js.status().is_success() {
                        if let Ok(text_js) = res_js.text().await {
                            if let Some(found) = Self::try_extract_spade(&text_js, &patterns) {
                                println!("get_spade_url: Found spade URL in settings.js: {}", &found[..found.len().min(80)]);
                                return Ok(found);
                            }
                            // Log a snippet of settings.js for debugging
                            let snippet: String = text_js.chars().take(500).collect();
                            println!("get_spade_url: settings.js snippet (no match): {}", snippet);
                        }
                    }
                }
                Err(e) => return Err(format!("Failed to fetch settings.js: {}", e)),
            }
        }

        Err("Spade URL not found in any pattern".to_string())
    }

    fn try_extract_spade(text: &str, patterns: &[&str]) -> Option<String> {
        for pattern in patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(text) {
                    return Some(caps[1].to_string());
                }
            }
        }
        None
    }

    pub async fn send_spade_minute_watched(&self, spade_url: &str, payload_data: Value) -> Result<bool, String> {
        let json_str = serde_json::to_string(&payload_data).map_err(|e| e.to_string())?;
        let b64_data = STANDARD.encode(json_str);
        
        let res = self.client.post(spade_url)
            .form(&[("data", b64_data)])
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = res.status().as_u16();
        let success = status == 204 || status == 200;
        
        if success {
            println!("spade_ping: OK ({})", status);
        } else {
            println!("spade_ping: FAILED status {} for url {}", status, &spade_url[..spade_url.len().min(80)]);
        }

        Ok(success)
    }

    /// Fetch real-time drop session progress for the current channel.
    /// Returns the GQL response containing `data.currentUser.dropCurrentSession`.
    pub async fn get_drop_current_session(&self, store: &Arc<Store<tauri::Wry>>, channel_login: &str) -> Result<Value, String> {
        let query = "
        query DropCurrentSessionContext($channelLogin: String!) {
            currentUser {
                id
                dropCurrentSession(channelLogin: $channelLogin) {
                    dropID
                    currentMinutesWatched
                    requiredMinutesWatched
                    drop {
                        id
                        name
                        requiredMinutesWatched
                        benefitEdges {
                            benefit {
                                id
                                name
                                imageAssetURL
                            }
                        }
                        campaign {
                            id
                            name
                            game {
                                id
                                displayName
                                boxArtURL
                            }
                        }
                    }
                }
            }
        }";
        let vars = json!({ "channelLogin": channel_login });
        self.post(store, "DropCurrentSessionContext", query, vars).await
    }

    pub async fn get_channel_points_context(&self, store: &Arc<Store<tauri::Wry>>, channel_login: &str) -> Result<Value, String> {
        let query = "
        query GetChannelPointsContext($login: String!) {
            channel(name: $login) {
                id
                displayName
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
        self.post(store, "GetChannelPointsContext", query, vars).await
    }

    pub async fn claim_community_points(&self, store: &Arc<Store<tauri::Wry>>, channel_id: &str, claim_id: &str) -> Result<Value, String> {
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
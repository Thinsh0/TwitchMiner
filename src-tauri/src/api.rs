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
        let url = format!("https://www.twitch.tv/{}", streamer_login);
        let res = self.client.get(&url).send().await.map_err(|e| e.to_string())?;
        let text = res.text().await.map_err(|e| e.to_string())?;

        let re = Regex::new(r#""beacon_?url": ?"(https://video-edge-[.\w\-/]+\.ts(?:\?allow_stream=true)?)""#).unwrap();
        if let Some(caps) = re.captures(&text) {
            return Ok(caps[1].to_string());
        }
        
        let re_settings = Regex::new(r#"src="(https://[\w.]+/config/settings\.[0-9a-f]{32}\.js)""#).unwrap();
        if let Some(caps) = re_settings.captures(&text) {
            let settings_url = &caps[1];
            let res_js = self.client.get(settings_url).send().await.map_err(|e| e.to_string())?;
            let text_js = res_js.text().await.map_err(|e| e.to_string())?;
            if let Some(caps_js) = re.captures(&text_js) {
                return Ok(caps_js[1].to_string());
            }
        }

        Err("Spade URL not found".to_string())
    }

    pub async fn send_spade_minute_watched(&self, spade_url: &str, payload_data: Value) -> Result<bool, String> {
        let json_str = serde_json::to_string(&payload_data).map_err(|e| e.to_string())?;
        let b64_data = STANDARD.encode(json_str);
        
        let res = self.client.post(spade_url)
            .form(&[("data", b64_data)])
            .send()
            .await
            .map_err(|e| e.to_string())?;

        Ok(res.status().as_u16() == 204)
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
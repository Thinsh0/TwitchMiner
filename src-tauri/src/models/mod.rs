use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MinerStatus {
    pub status: String,
    pub is_running: bool,
    pub current_streamer: Option<String>,
    pub current_drop: Option<String>,
    pub drop_image_url: Option<String>,
    pub streamer_image_url: Option<String>,
    pub progress: f64,
    pub current_points: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AuthInitResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

// We will add more structs for GQL responses as we implement them.
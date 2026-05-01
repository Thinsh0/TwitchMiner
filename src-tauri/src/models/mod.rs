use serde::{Deserialize, Serialize};

/// Snapshot of a single time-based drop that is currently being mined.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ActiveDropInfo {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub current_minutes: i64,
    pub required_minutes: i64,
    /// Individual progress percentage (0-100).
    pub progress: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MinerStatus {
    pub status: String,
    pub is_running: bool,
    pub current_streamer: Option<String>,
    pub current_streamer_login: Option<String>,
    /// Name of the first active drop (kept for backwards-compat).
    pub current_drop: Option<String>,
    /// Image of the first active drop (kept for backwards-compat).
    pub drop_image_url: Option<String>,
    pub streamer_image_url: Option<String>,
    /// Overall progress across all active drops (kept for backwards-compat).
    pub progress: f64,
    pub current_points: String,
    /// Full list of drops currently being mined (may be >1 simultaneous campaigns).
    pub active_drops: Vec<ActiveDropInfo>,
    pub started_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DropCampaign {
    pub id: String,
    pub name: String,
    pub game_name: String,
    pub image_url: Option<String>,
    pub status: String,
    pub start_date: String,
    pub end_date: String,
    pub drops: Vec<DropItem>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DropItem {
    pub id: String,
    pub name: String,
    pub required_minutes: u64,
    pub current_minutes: u64,
    pub is_claimed: bool,
    pub image_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ChannelPointsInfo {
    pub channel_name: String,
    pub points: u64,
    pub has_claim_available: bool,
    pub status: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct StreakMinerStatus {
    pub status: String,
    pub current_channels: Vec<String>,
    pub total_pings_sent: u64,
    pub successful_pings: u64,
    pub failed_pings: u64,
    pub cycle_count: u64,
    pub channels_info: Vec<ChannelPointsInfo>,
    pub started_at: Option<String>,
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
    #[serde(default)]
    pub id_token: Option<String>,
    pub refresh_token: String,
    pub expires_in: u64,
    #[serde(default)]
    pub scope: Vec<String>,
    pub token_type: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppConfig {
    pub client_id: String,
    pub priorities: Priorities,
    pub bonus: BonusConfig,
    pub settings: Settings,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Priorities {
    pub games: Vec<String>,
    pub streamers: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct BonusConfig {
    pub daily_streaks: DailyStreaksConfig,
    pub points_farming: PointsFarmingConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DailyStreaksConfig {
    pub channels: Vec<String>,
    pub auto_start: bool,
    pub mode: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PointsFarmingConfig {
    pub channels: Vec<String>,
    pub auto_start: bool,
    pub max_concurrent: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Settings {
    pub language: String,
    pub check_interval: u64,
    pub notifications_enabled: bool,
    pub notify_active_drop: bool,
    pub notify_claim: bool,
    pub notify_campaign_finish: bool,
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    #[serde(default = "default_true")]
    pub show_tray_icon: bool,
    #[serde(default)]
    pub auto_start_drops: bool,
}

fn default_true() -> bool {
    true
}

// We will add more structs for GQL responses as we implement them.

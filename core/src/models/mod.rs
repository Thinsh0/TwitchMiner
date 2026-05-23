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
    /// RFC3339 timestamp of when the drop was claimed (None for legacy entries).
    #[serde(default)]
    pub claimed_at: Option<String>,
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScheduleDay {
    pub enabled: bool,
    pub start_time: String,
    pub end_time: String,
}

impl Default for ScheduleDay {
    fn default() -> Self {
        Self {
            enabled: false,
            start_time: "08:00".to_string(),
            end_time: "18:00".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ScheduleException {
    pub date: String,
    pub is_allowed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkSchedule {
    pub enabled: bool,
    pub monday: ScheduleDay,
    pub tuesday: ScheduleDay,
    pub wednesday: ScheduleDay,
    pub thursday: ScheduleDay,
    pub friday: ScheduleDay,
    pub saturday: ScheduleDay,
    pub sunday: ScheduleDay,
    pub exceptions: Vec<ScheduleException>,
}

impl Default for WorkSchedule {
    fn default() -> Self {
        Self {
            enabled: false,
            monday: ScheduleDay::default(),
            tuesday: ScheduleDay::default(),
            wednesday: ScheduleDay::default(),
            thursday: ScheduleDay::default(),
            friday: ScheduleDay::default(),
            saturday: ScheduleDay::default(),
            sunday: ScheduleDay::default(),
            exceptions: Vec::new(),
        }
    }
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
    #[serde(default)]
    pub mine_all_inventory: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppConfig {
    pub client_id: String,
    pub priorities: Priorities,
    pub bonus: BonusConfig,
    pub settings: Settings,
    #[serde(default)]
    pub schedule: WorkSchedule,
}

pub fn is_mining_allowed(schedule: &WorkSchedule) -> bool {
    if !schedule.enabled {
        return true;
    }
    let local_time = chrono::Local::now();
    let date_str = local_time.format("%Y-%m-%d").to_string();

    // Check exceptions first
    if let Some(exception) = schedule.exceptions.iter().find(|e| e.date == date_str) {
        return exception.is_allowed;
    }

    // Check day configuration
    use chrono::Datelike;
    let weekday = local_time.weekday();
    let day_config = match weekday {
        chrono::Weekday::Mon => &schedule.monday,
        chrono::Weekday::Tue => &schedule.tuesday,
        chrono::Weekday::Wed => &schedule.wednesday,
        chrono::Weekday::Thu => &schedule.thursday,
        chrono::Weekday::Fri => &schedule.friday,
        chrono::Weekday::Sat => &schedule.saturday,
        chrono::Weekday::Sun => &schedule.sunday,
    };

    if day_config.enabled {
        let current_hm = local_time.format("%H:%M").to_string();
        current_hm >= day_config.start_time && current_hm <= day_config.end_time
    } else {
        false
    }
}

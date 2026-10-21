//! Every change to the application state is an [`Event`].
//!
//! Events are the only way state changes: producers call `Store::emit`, the store
//! applies them with [`crate::state::apply`], persists the ones worth keeping and
//! broadcasts them. A client mirrors the state by taking one snapshot and then
//! applying the events it receives with the same reducer.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tm_twitch::models::{Campaign, StreamInfo};

use crate::state::{
    AccountInfo, AccountStatus, ChannelMeta, DropAlertInfo, DropTarget, LoginFlow, ModuleHealth,
    Notice,
};

/// An event with its position in the global stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Strictly increasing sequence number (state version after applying the event).
    pub seq: u64,
    pub ts: DateTime<Utc>,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    // ------------------------------------------------------------------ engine
    EngineStarted {
        version: String,
        #[serde(default)]
        instance_id: String,
    },
    SettingsChanged {
        scope: String,
        value: Value,
    },
    ManifestUpdated {
        revision: u32,
    },
    /// Watching is allowed right now (not paused, inside the watching hours), judged
    /// by the engine in its own time zone.
    WatchingAllowed {
        allowed: bool,
    },
    NoticeRaised {
        notice: Notice,
    },
    NoticeCleared {
        id: String,
    },
    LoginStarted {
        flow: LoginFlow,
    },
    LoginFinished {
        flow_id: String,
        account: Option<String>,
        error: Option<String>,
    },

    // ----------------------------------------------------------------- account
    AccountUpserted {
        info: AccountInfo,
    },
    AccountRemoved {
        account: String,
    },
    AccountStatusChanged {
        account: String,
        status: AccountStatus,
    },
    ModuleHealthChanged {
        account: String,
        module: String,
        health: ModuleHealth,
    },
    /// Free-form, module-owned data (lets new modules expose state without touching the core).
    ModuleDataSet {
        account: String,
        module: String,
        key: String,
        value: Value,
    },

    // ---------------------------------------------------------------- channels
    /// A module tracks a channel (`source` = module id). Metadata is merged.
    ChannelTracked {
        account: String,
        channel: ChannelMeta,
        source: String,
    },
    /// A module no longer needs the channel; it disappears once nobody tracks it.
    ChannelReleased {
        account: String,
        channel_id: String,
        source: String,
    },
    StreamChanged {
        account: String,
        channel_id: String,
        stream: Option<StreamInfo>,
    },
    ViewersChanged {
        account: String,
        channel_id: String,
        viewers: u64,
    },

    // ------------------------------------------------------------------ points
    PointsBalance {
        account: String,
        channel_id: String,
        balance: u64,
        multiplier: f64,
    },
    PointsEarned {
        account: String,
        channel_id: String,
        amount: i64,
        reason: String,
        balance: u64,
    },
    PointsSpent {
        account: String,
        channel_id: String,
        amount: i64,
        balance: u64,
    },
    RaidJoined {
        account: String,
        channel_id: String,
        target_login: String,
    },
    MomentClaimed {
        account: String,
        channel_id: String,
    },
    PredictionPlaced {
        account: String,
        channel_id: String,
        event_id: String,
        title: String,
        outcome: String,
        points: u64,
    },
    PredictionResult {
        account: String,
        channel_id: String,
        event_id: String,
        result: String,
        points_won: i64,
    },

    // ----------------------------------------------------------------- watching
    WatchStarted {
        account: String,
        channel_id: String,
        reason: String,
    },
    WatchStopped {
        account: String,
        channel_id: String,
        cause: String,
    },
    /// One minute of watch time was credited by Twitch's tracker.
    WatchMinute {
        account: String,
        channel_id: String,
    },

    // ------------------------------------------------------------------- drops
    CampaignsReplaced {
        account: String,
        campaigns: Vec<Campaign>,
    },
    DropProgress {
        account: String,
        campaign_id: String,
        drop_id: String,
        current_minutes: u32,
        required_minutes: u32,
    },
    DropClaimed {
        account: String,
        campaign_id: String,
        drop_id: String,
        drop_name: String,
        game: String,
    },
    DropTargetChanged {
        account: String,
        target: Option<DropTarget>,
    },
    /// Something about a wanted campaign needs the user's attention (sent once).
    DropAlert {
        account: String,
        alert: DropAlertInfo,
    },
}

impl Event {
    pub fn kind(&self) -> &'static str {
        match self {
            Event::EngineStarted { .. } => "engine_started",
            Event::SettingsChanged { .. } => "settings_changed",
            Event::ManifestUpdated { .. } => "manifest_updated",
            Event::WatchingAllowed { .. } => "watching_allowed",
            Event::NoticeRaised { .. } => "notice_raised",
            Event::NoticeCleared { .. } => "notice_cleared",
            Event::LoginStarted { .. } => "login_started",
            Event::LoginFinished { .. } => "login_finished",
            Event::AccountUpserted { .. } => "account_upserted",
            Event::AccountRemoved { .. } => "account_removed",
            Event::AccountStatusChanged { .. } => "account_status_changed",
            Event::ModuleHealthChanged { .. } => "module_health_changed",
            Event::ModuleDataSet { .. } => "module_data_set",
            Event::ChannelTracked { .. } => "channel_tracked",
            Event::ChannelReleased { .. } => "channel_released",
            Event::StreamChanged { .. } => "stream_changed",
            Event::ViewersChanged { .. } => "viewers_changed",
            Event::PointsBalance { .. } => "points_balance",
            Event::PointsEarned { .. } => "points_earned",
            Event::PointsSpent { .. } => "points_spent",
            Event::RaidJoined { .. } => "raid_joined",
            Event::MomentClaimed { .. } => "moment_claimed",
            Event::PredictionPlaced { .. } => "prediction_placed",
            Event::PredictionResult { .. } => "prediction_result",
            Event::WatchStarted { .. } => "watch_started",
            Event::WatchStopped { .. } => "watch_stopped",
            Event::WatchMinute { .. } => "watch_minute",
            Event::CampaignsReplaced { .. } => "campaigns_replaced",
            Event::DropProgress { .. } => "drop_progress",
            Event::DropClaimed { .. } => "drop_claimed",
            Event::DropTargetChanged { .. } => "drop_target_changed",
            Event::DropAlert { .. } => "drop_alert",
        }
    }

    pub fn account(&self) -> Option<&str> {
        match self {
            Event::AccountRemoved { account }
            | Event::AccountStatusChanged { account, .. }
            | Event::ModuleHealthChanged { account, .. }
            | Event::ModuleDataSet { account, .. }
            | Event::ChannelTracked { account, .. }
            | Event::ChannelReleased { account, .. }
            | Event::StreamChanged { account, .. }
            | Event::ViewersChanged { account, .. }
            | Event::PointsBalance { account, .. }
            | Event::PointsEarned { account, .. }
            | Event::PointsSpent { account, .. }
            | Event::RaidJoined { account, .. }
            | Event::MomentClaimed { account, .. }
            | Event::PredictionPlaced { account, .. }
            | Event::PredictionResult { account, .. }
            | Event::WatchStarted { account, .. }
            | Event::WatchStopped { account, .. }
            | Event::WatchMinute { account, .. }
            | Event::CampaignsReplaced { account, .. }
            | Event::DropProgress { account, .. }
            | Event::DropClaimed { account, .. }
            | Event::DropTargetChanged { account, .. }
            | Event::DropAlert { account, .. } => Some(account),
            Event::AccountUpserted { info } => Some(&info.id),
            _ => None,
        }
    }

    pub fn channel_id(&self) -> Option<&str> {
        match self {
            Event::StreamChanged { channel_id, .. }
            | Event::ChannelReleased { channel_id, .. }
            | Event::ViewersChanged { channel_id, .. }
            | Event::PointsBalance { channel_id, .. }
            | Event::PointsEarned { channel_id, .. }
            | Event::PointsSpent { channel_id, .. }
            | Event::RaidJoined { channel_id, .. }
            | Event::MomentClaimed { channel_id, .. }
            | Event::PredictionPlaced { channel_id, .. }
            | Event::PredictionResult { channel_id, .. }
            | Event::WatchStarted { channel_id, .. }
            | Event::WatchStopped { channel_id, .. }
            | Event::WatchMinute { channel_id, .. } => Some(channel_id),
            Event::ChannelTracked { channel, .. } => Some(&channel.id),
            _ => None,
        }
    }

    /// Events kept in the history table (the rest is either derivable or too chatty).
    pub fn is_historical(&self) -> bool {
        matches!(
            self,
            Event::PointsEarned { .. }
                | Event::PointsSpent { .. }
                | Event::RaidJoined { .. }
                | Event::MomentClaimed { .. }
                | Event::PredictionPlaced { .. }
                | Event::PredictionResult { .. }
                | Event::WatchStarted { .. }
                | Event::WatchStopped { .. }
                | Event::DropClaimed { .. }
                | Event::DropAlert { .. }
                | Event::AccountStatusChanged { .. }
                | Event::NoticeRaised { .. }
                | Event::LoginFinished { .. }
        )
    }
}

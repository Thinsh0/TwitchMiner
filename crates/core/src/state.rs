//! The single source of truth: one [`State`] value, changed only through [`apply`].
//!
//! The reducer is pure and deterministic so that any client (UI, bot, remote
//! dashboard) can keep an exact mirror: snapshot + replay of [`Event`]s.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tm_twitch::models::{Campaign, StreamInfo};

use crate::event::{Envelope, Event};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct State {
    /// Sequence number of the last applied event.
    pub seq: u64,
    pub version: String,
    /// Stable id of this engine instance (lets peers recognise each other).
    #[serde(default)]
    pub instance_id: String,
    pub started_at: Option<DateTime<Utc>>,
    pub manifest_revision: u32,
    /// Watching allowed now, in the engine's time zone (`None`: older engine).
    #[serde(default)]
    pub watching_allowed: Option<bool>,
    /// Raw settings documents by scope (`engine`, `module:<id>`, `account:<id>/module:<id>`).
    pub settings: BTreeMap<String, Value>,
    pub notices: BTreeMap<String, Notice>,
    pub logins: BTreeMap<String, LoginFlow>,
    pub accounts: BTreeMap<String, Account>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Notice {
    pub id: String,
    pub level: Level,
    /// English fallback text; interfaces build their own from `kind` + `params`.
    pub message: String,
    pub at: DateTime<Utc>,
    /// Stable type: `update`, `duplicate`, `proxy_invalid`. Absent from older engines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Values for the interface's own text and action, e.g. `{"version", "url"}`.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoginFlow {
    pub flow_id: String,
    pub user_code: String,
    pub verification_uri: String,
    /// `verification_uri` with the code already filled in (absent from older engines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_uri_complete: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountInfo {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AccountStatus {
    Stopped,
    Starting,
    Running,
    /// Token revoked or expired: the user must log in again.
    AuthRequired,
    /// Left to another instance (`peer`) that is mining this account.
    Standby {
        peer: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModuleHealth {
    Disabled,
    Running,
    /// Crashed and waiting to be restarted.
    Restarting {
        error: String,
        attempt: u32,
    },
    Stopped,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub info: Option<AccountInfo>,
    pub status: Option<AccountStatus>,
    pub modules: BTreeMap<String, ModuleHealth>,
    pub channels: BTreeMap<String, Channel>,
    pub campaigns: BTreeMap<String, Campaign>,
    pub drop_target: Option<DropTarget>,
    pub watching: BTreeMap<String, WatchSlot>,
    pub session: SessionStats,
    pub module_data: BTreeMap<String, BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelMeta {
    pub id: String,
    pub login: String,
    pub display_name: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Channel {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub stream: Option<StreamInfo>,
    pub points: Option<u64>,
    pub multiplier: f64,
    /// Modules tracking this channel.
    pub sources: BTreeSet<String>,
    /// Points earned on this channel since the engine started.
    pub session_points: i64,
    /// Minutes credited on this channel since the engine started.
    pub session_minutes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchSlot {
    pub channel_id: String,
    pub reason: String,
    pub since: DateTime<Utc>,
    pub minutes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DropTarget {
    pub campaign_id: String,
    pub drop_id: Option<String>,
    pub channel_id: String,
    pub game: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DropAlertKind {
    /// Fits, but with less than the safety margin.
    AtRisk,
    /// Cannot be completed before it ends.
    WillMiss,
    /// A wanted game's account is not linked.
    NotLinked,
    /// A campaign appeared for a wanted game.
    NewCampaign,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DropAlertInfo {
    pub kind: DropAlertKind,
    pub campaign_id: String,
    pub campaign: String,
    pub game: String,
    pub ends_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_url: Option<String>,
    /// Rewards of the campaign's objective.
    #[serde(default)]
    pub rewards: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionStats {
    pub points_earned: i64,
    pub watch_minutes: u64,
    pub drops_claimed: u64,
    pub bonuses_claimed: u64,
    pub raids_joined: u64,
    pub moments_claimed: u64,
}

impl State {
    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.get(id)
    }
}

impl Channel {
    fn from_meta(m: ChannelMeta) -> Self {
        Channel {
            id: m.id,
            login: m.login,
            display_name: m.display_name,
            avatar_url: m.avatar_url,
            stream: None,
            points: None,
            multiplier: 1.0,
            sources: BTreeSet::new(),
            session_points: 0,
            session_minutes: 0,
        }
    }
}

/// Applies `env` to `state`. Returns `false` (and leaves `state` untouched) when
/// the event would not change anything; such events are dropped by the store.
pub fn apply_envelope(state: &mut State, env: &Envelope) -> bool {
    let changed = apply(state, &env.event, env.ts);
    if changed {
        state.seq = env.seq;
    }
    changed
}

/// The reducer. `now` is the event timestamp (never the wall clock, to stay replayable).
pub fn apply(state: &mut State, event: &Event, now: DateTime<Utc>) -> bool {
    use Event::*;
    match event {
        EngineStarted {
            version,
            instance_id,
        } => {
            state.version = version.clone();
            state.instance_id = instance_id.clone();
            state.started_at = Some(now);
            true
        }
        SettingsChanged { scope, value } => {
            if value.is_null() {
                return state.settings.remove(scope).is_some();
            }
            set_if_changed(state.settings.entry(scope.clone()).or_default(), value)
        }
        ManifestUpdated { revision } => set_if_changed(&mut state.manifest_revision, revision),
        WatchingAllowed { allowed } => set_if_changed(&mut state.watching_allowed, &Some(*allowed)),
        NoticeRaised { notice } => {
            if state.notices.get(&notice.id).is_some_and(|n| {
                n.level == notice.level
                    && n.message == notice.message
                    && n.kind == notice.kind
                    && n.params == notice.params
            }) {
                return false;
            }
            state.notices.insert(notice.id.clone(), notice.clone());
            true
        }
        NoticeCleared { id } => state.notices.remove(id).is_some(),
        LoginStarted { flow } => {
            state.logins.insert(flow.flow_id.clone(), flow.clone());
            true
        }
        LoginFinished { flow_id, .. } => state.logins.remove(flow_id).is_some(),

        AccountUpserted { info } => {
            let acc = state.accounts.entry(info.id.clone()).or_default();
            set_if_changed(&mut acc.info, &Some(info.clone()))
        }
        AccountRemoved { account } => state.accounts.remove(account).is_some(),
        AccountStatusChanged { account, status } => {
            let Some(acc) = state.accounts.get_mut(account) else {
                return false;
            };
            let changed = set_if_changed(&mut acc.status, &Some(status.clone()));
            if changed && !matches!(status, AccountStatus::Running) {
                acc.watching.clear();
            }
            changed
        }
        ModuleHealthChanged {
            account,
            module,
            health,
        } => with_account(state, account, |acc| {
            set_if_changed(
                acc.modules
                    .entry(module.clone())
                    .or_insert(ModuleHealth::Stopped),
                health,
            )
        }),
        ModuleDataSet {
            account,
            module,
            key,
            value,
        } => with_account(state, account, |acc| {
            let m = acc.module_data.entry(module.clone()).or_default();
            if value.is_null() {
                return m.remove(key).is_some();
            }
            set_if_changed(m.entry(key.clone()).or_default(), value)
        }),

        ChannelTracked {
            account,
            channel,
            source,
        } => with_account(state, account, |acc| {
            let ch = acc
                .channels
                .entry(channel.id.clone())
                .or_insert_with(|| Channel::from_meta(channel.clone()));
            let mut changed = ch.sources.insert(source.clone());
            if !channel.login.is_empty() && ch.login != channel.login {
                ch.login = channel.login.clone();
                changed = true;
            }
            if !channel.display_name.is_empty() && ch.display_name != channel.display_name {
                ch.display_name = channel.display_name.clone();
                changed = true;
            }
            if channel.avatar_url.is_some() && ch.avatar_url != channel.avatar_url {
                ch.avatar_url = channel.avatar_url.clone();
                changed = true;
            }
            changed
        }),
        ChannelReleased {
            account,
            channel_id,
            source,
        } => with_account(state, account, |acc| {
            let Some(ch) = acc.channels.get_mut(channel_id) else {
                return false;
            };
            let changed = ch.sources.remove(source);
            if ch.sources.is_empty() && !acc.watching.contains_key(channel_id) {
                acc.channels.remove(channel_id);
            }
            changed
        }),
        StreamChanged {
            account,
            channel_id,
            stream,
        } => with_channel(state, account, channel_id, |ch| {
            set_if_changed(&mut ch.stream, stream)
        }),
        ViewersChanged {
            account,
            channel_id,
            viewers,
        } => with_channel(state, account, channel_id, |ch| match ch.stream.as_mut() {
            Some(s) => set_if_changed(&mut s.viewers, viewers),
            None => false,
        }),

        PointsBalance {
            account,
            channel_id,
            balance,
            multiplier,
        } => with_channel(state, account, channel_id, |ch| {
            set_if_changed(&mut ch.points, &Some(*balance))
                | set_if_changed(&mut ch.multiplier, multiplier)
        }),
        PointsEarned {
            account,
            channel_id,
            amount,
            reason,
            balance,
        } => with_account(state, account, |acc| {
            acc.session.points_earned += amount;
            if reason == "CLAIM" {
                acc.session.bonuses_claimed += 1;
            }
            if let Some(ch) = acc.channels.get_mut(channel_id) {
                ch.points = Some(*balance);
                ch.session_points += amount;
            }
            true
        }),
        PointsSpent {
            account,
            channel_id,
            balance,
            ..
        } => with_channel(state, account, channel_id, |ch| {
            set_if_changed(&mut ch.points, &Some(*balance))
        }),
        RaidJoined { account, .. } => with_account(state, account, |acc| {
            acc.session.raids_joined += 1;
            true
        }),
        MomentClaimed { account, .. } => with_account(state, account, |acc| {
            acc.session.moments_claimed += 1;
            true
        }),
        PredictionPlaced { .. } | PredictionResult { .. } => true,

        WatchStarted {
            account,
            channel_id,
            reason,
        } => with_account(state, account, |acc| {
            acc.watching.insert(
                channel_id.clone(),
                WatchSlot {
                    channel_id: channel_id.clone(),
                    reason: reason.clone(),
                    since: now,
                    minutes: 0,
                },
            );
            true
        }),
        WatchStopped {
            account,
            channel_id,
            ..
        } => with_account(state, account, |acc| {
            let removed = acc.watching.remove(channel_id).is_some();
            if acc
                .channels
                .get(channel_id)
                .is_some_and(|c| c.sources.is_empty())
            {
                acc.channels.remove(channel_id);
            }
            removed
        }),
        WatchMinute {
            account,
            channel_id,
        } => with_account(state, account, |acc| {
            acc.session.watch_minutes += 1;
            if let Some(w) = acc.watching.get_mut(channel_id) {
                w.minutes += 1;
            }
            if let Some(ch) = acc.channels.get_mut(channel_id) {
                ch.session_minutes += 1;
            }
            true
        }),

        CampaignsReplaced { account, campaigns } => with_account(state, account, |acc| {
            let next: BTreeMap<String, Campaign> = campaigns
                .iter()
                .map(|c| (c.id.clone(), c.clone()))
                .collect();
            set_if_changed(&mut acc.campaigns, &next)
        }),
        DropProgress {
            account,
            campaign_id,
            drop_id,
            current_minutes,
            required_minutes,
        } => with_account(state, account, |acc| {
            let Some(drop) = acc
                .campaigns
                .get_mut(campaign_id)
                .and_then(|c| c.drops.iter_mut().find(|d| &d.id == drop_id))
            else {
                return false;
            };
            let mut changed = set_if_changed(&mut drop.current_minutes, current_minutes);
            if *required_minutes > 0 {
                changed |= set_if_changed(&mut drop.required_minutes, required_minutes);
            }
            changed
        }),
        DropClaimed {
            account,
            campaign_id,
            drop_id,
            ..
        } => with_account(state, account, |acc| {
            acc.session.drops_claimed += 1;
            if let Some(d) = acc
                .campaigns
                .get_mut(campaign_id)
                .and_then(|c| c.drops.iter_mut().find(|d| &d.id == drop_id))
            {
                d.claimed = true;
                d.current_minutes = d.current_minutes.max(d.required_minutes);
            }
            true
        }),
        DropTargetChanged { account, target } => with_account(state, account, |acc| {
            set_if_changed(&mut acc.drop_target, target)
        }),
        // A notification: the plan itself lives in `module_data.drops.plan`.
        DropAlert { account, .. } => state.accounts.contains_key(account),
    }
}

fn set_if_changed<T: PartialEq + Clone>(slot: &mut T, value: &T) -> bool {
    if slot == value {
        false
    } else {
        *slot = value.clone();
        true
    }
}

fn with_account(state: &mut State, id: &str, f: impl FnOnce(&mut Account) -> bool) -> bool {
    state.accounts.get_mut(id).is_some_and(f)
}

fn with_channel(
    state: &mut State,
    account: &str,
    channel: &str,
    f: impl FnOnce(&mut Channel) -> bool,
) -> bool {
    state
        .accounts
        .get_mut(account)
        .and_then(|a| a.channels.get_mut(channel))
        .is_some_and(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc_state() -> State {
        let mut s = State::default();
        let info = AccountInfo {
            id: "1".into(),
            login: "me".into(),
            display_name: "Me".into(),
            avatar_url: None,
            enabled: true,
        };
        assert!(apply(&mut s, &Event::AccountUpserted { info }, Utc::now()));
        s
    }

    fn track(s: &mut State, id: &str, source: &str) -> bool {
        let channel = ChannelMeta {
            id: id.into(),
            login: format!("c{id}"),
            display_name: format!("C{id}"),
            avatar_url: None,
        };
        apply(
            s,
            &Event::ChannelTracked {
                account: "1".into(),
                channel,
                source: source.into(),
            },
            Utc::now(),
        )
    }

    #[test]
    fn channel_lifecycle_by_sources() {
        let mut s = acc_state();
        assert!(track(&mut s, "9", "points"));
        assert!(track(&mut s, "9", "drops"));
        assert!(!track(&mut s, "9", "drops"), "duplicate track is a no-op");
        let rel = |src: &str| Event::ChannelReleased {
            account: "1".into(),
            channel_id: "9".into(),
            source: src.into(),
        };
        assert!(apply(&mut s, &rel("points"), Utc::now()));
        assert!(s.accounts["1"].channels.contains_key("9"));
        assert!(apply(&mut s, &rel("drops"), Utc::now()));
        assert!(!s.accounts["1"].channels.contains_key("9"));
    }

    #[test]
    fn points_accumulate() {
        let mut s = acc_state();
        track(&mut s, "9", "points");
        let e = Event::PointsEarned {
            account: "1".into(),
            channel_id: "9".into(),
            amount: 50,
            reason: "CLAIM".into(),
            balance: 150,
        };
        apply(&mut s, &e, Utc::now());
        apply(&mut s, &e, Utc::now());
        let a = &s.accounts["1"];
        assert_eq!(a.session.points_earned, 100);
        assert_eq!(a.session.bonuses_claimed, 2);
        assert_eq!(a.channels["9"].points, Some(150));
    }

    #[test]
    fn unchanged_settings_are_noop() {
        let mut s = State::default();
        let e = Event::SettingsChanged {
            scope: "engine".into(),
            value: serde_json::json!({"a":1}),
        };
        assert!(apply(&mut s, &e, Utc::now()));
        assert!(!apply(&mut s, &e, Utc::now()));
    }
}

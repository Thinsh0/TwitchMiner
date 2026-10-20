//! Settings live in the state (`State::settings`) as JSON documents keyed by scope:
//!
//! - `engine`: [`EngineSettings`]
//! - `module:<id>`: module settings shared by all accounts
//! - `account:<account>/module:<id>`: per-account overrides
//!
//! A module's effective settings are `defaults ⊕ module scope ⊕ account scope`
//! (deep merge), so overrides only need to contain what differs.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use tm_twitch::manifest::merge_json;

use crate::state::State;

pub const ENGINE_SCOPE: &str = "engine";

pub fn module_scope(module: &str) -> String {
    format!("module:{module}")
}

pub fn account_module_scope(account: &str, module: &str) -> String {
    format!("account:{account}/module:{module}")
}

/// Validates a scope string and returns what it addresses.
pub enum Scope<'a> {
    Engine,
    Module(&'a str),
    AccountModule { account: &'a str, module: &'a str },
}

pub fn parse_scope(scope: &str) -> Option<Scope<'_>> {
    if scope == ENGINE_SCOPE {
        return Some(Scope::Engine);
    }
    if let Some(m) = scope.strip_prefix("module:") {
        return (!m.is_empty()).then_some(Scope::Module(m));
    }
    let rest = scope.strip_prefix("account:")?;
    let (account, module) = rest.split_once("/module:")?;
    (!account.is_empty() && !module.is_empty()).then_some(Scope::AccountModule { account, module })
}

pub const DEFAULT_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/Thinsh0/TwitchMiner/HEAD/manifest/twitch.json";

pub const DEFAULT_RELEASES_URL: &str =
    "https://api.github.com/repos/Thinsh0/TwitchMiner/releases/latest";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(default)]
pub struct EngineSettings {
    /// Channels watched at the same time per account (Twitch credits at most 2).
    pub max_watch_slots: u8,
    /// HTTP proxy for every connection (`http://user:pass@host:port`).
    pub proxy: Option<String>,
    /// Where to look for manifest updates (GQL hashes, client ids).
    pub manifest_url: Option<String>,
    pub manifest_auto_update: bool,
    /// History rows older than this are deleted (statistics are kept forever).
    pub history_retention_days: u32,
    /// Check GitHub releases for a newer version of the application.
    pub update_check: bool,
    pub releases_url: String,
    /// Global pause: nothing is watched (modules keep tracking and claiming).
    pub paused: bool,
    /// When non-empty, watching only happens inside these local-time windows.
    pub active_hours: Vec<TimeWindow>,
    /// `secondary` instances leave an account to any reachable peer already mining it
    /// (and take over when that peer goes away). `primary` instances only warn.
    pub role: Role,
    /// Other TwitchMiner instances (e.g. a home server reachable through Tailscale).
    pub peers: Vec<Peer>,
    /// AI agent access: the MCP endpoint and the agent token. Off = both refused.
    pub agent_enabled: bool,
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct Peer {
    /// Base URL of the peer's API, e.g. `http://100.64.0.2:7878`.
    pub url: String,
    /// The peer's API token (`twitchminer api-token` on that machine).
    #[serde(default)]
    pub token: Option<String>,
    /// Display name (defaults to the URL).
    #[serde(default)]
    pub name: Option<String>,
}

impl Peer {
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.url)
    }
}

/// A daily time window in local time. `end` before `start` spans midnight.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct TimeWindow {
    /// ISO weekdays (1 = Monday … 7 = Sunday) on which the window *starts*; empty = every day.
    #[serde(default)]
    pub days: Vec<u8>,
    /// `HH:MM`
    pub start: String,
    /// `HH:MM`
    pub end: String,
}

fn hm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl TimeWindow {
    pub fn validate(&self) -> Result<(), String> {
        hm(&self.start).ok_or_else(|| format!("bad start time `{}`", self.start))?;
        hm(&self.end).ok_or_else(|| format!("bad end time `{}`", self.end))?;
        if self.days.iter().any(|d| !(1..=7).contains(d)) {
            return Err("days must be between 1 (Monday) and 7 (Sunday)".into());
        }
        Ok(())
    }

    /// `weekday`: 1 = Monday … 7 = Sunday; `minute`: minutes since local midnight.
    pub fn contains(&self, weekday: u8, minute: u32) -> bool {
        let (Some(start), Some(end)) = (hm(&self.start), hm(&self.end)) else {
            return false;
        };
        let day_ok = |d: u8| self.days.is_empty() || self.days.contains(&d);
        if start <= end {
            day_ok(weekday) && minute >= start && minute < end
        } else {
            // Overnight: today after start, or tomorrow-morning part of yesterday's window.
            let yesterday = if weekday == 1 { 7 } else { weekday - 1 };
            (day_ok(weekday) && minute >= start) || (day_ok(yesterday) && minute < end)
        }
    }
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            max_watch_slots: 2,
            proxy: None,
            manifest_url: Some(DEFAULT_MANIFEST_URL.to_string()),
            manifest_auto_update: true,
            history_retention_days: 180,
            update_check: true,
            releases_url: DEFAULT_RELEASES_URL.to_string(),
            paused: false,
            active_hours: Vec::new(),
            role: Role::Primary,
            peers: Vec::new(),
            agent_enabled: false,
        }
    }
}

impl EngineSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=2).contains(&self.max_watch_slots) {
            return Err("max_watch_slots must be 1 or 2".into());
        }
        for p in &self.peers {
            if !(p.url.starts_with("http://") || p.url.starts_with("https://")) {
                return Err(format!(
                    "peer url must start with http:// or https:// (`{}`)",
                    p.url
                ));
            }
        }
        self.active_hours.iter().try_for_each(TimeWindow::validate)
    }

    /// Whether watching is allowed at local time `now`.
    pub fn watching_allowed(&self, now: chrono::DateTime<chrono::Local>) -> bool {
        use chrono::{Datelike, Timelike};
        if self.paused {
            return false;
        }
        if self.active_hours.is_empty() {
            return true;
        }
        let weekday = now.weekday().number_from_monday() as u8;
        let minute = now.hour() * 60 + now.minute();
        self.active_hours
            .iter()
            .any(|w| w.contains(weekday, minute))
    }

    pub fn from_state(state: &State) -> Self {
        typed(&effective_raw(
            Some(&serde_json::to_value(Self::default()).unwrap_or_default()),
            &[state.settings.get(ENGINE_SCOPE)],
        ))
        .unwrap_or_default()
    }
}

/// Effective settings of `module` for `account`.
pub fn module_effective(state: &State, account: &str, module: &str, defaults: &Value) -> Value {
    effective_raw(
        Some(defaults),
        &[
            state.settings.get(&module_scope(module)),
            state.settings.get(&account_module_scope(account, module)),
        ],
    )
}

fn effective_raw(defaults: Option<&Value>, layers: &[Option<&Value>]) -> Value {
    let mut v = defaults
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    for layer in layers.iter().flatten() {
        merge_json(&mut v, layer);
    }
    v
}

pub fn typed<T: DeserializeOwned>(v: &Value) -> Result<T, serde_json::Error> {
    serde_json::from_value(v.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn layering() {
        let mut s = State::default();
        s.settings
            .insert("module:points".into(), json!({"a": 2, "o": {"x": 1}}));
        s.settings
            .insert("account:7/module:points".into(), json!({"o": {"y": 2}}));
        let v = module_effective(&s, "7", "points", &json!({"a": 1, "b": 1}));
        assert_eq!(v, json!({"a": 2, "b": 1, "o": {"x": 1, "y": 2}}));
        let other = module_effective(&s, "8", "points", &json!({"a": 1}));
        assert_eq!(other, json!({"a": 2, "o": {"x": 1}}));
    }

    #[test]
    fn scopes() {
        assert!(matches!(parse_scope("engine"), Some(Scope::Engine)));
        assert!(matches!(
            parse_scope("module:drops"),
            Some(Scope::Module("drops"))
        ));
        assert!(matches!(
            parse_scope("account:1/module:drops"),
            Some(Scope::AccountModule {
                account: "1",
                module: "drops"
            })
        ));
        assert!(parse_scope("account:1").is_none());
    }

    #[test]
    fn time_windows() {
        let day = TimeWindow {
            days: vec![],
            start: "08:00".into(),
            end: "23:30".into(),
        };
        assert!(day.contains(3, 8 * 60));
        assert!(!day.contains(3, 23 * 60 + 30));
        let night = TimeWindow {
            days: vec![5],
            start: "22:00".into(),
            end: "06:00".into(),
        };
        assert!(night.contains(5, 23 * 60), "friday 23:00");
        assert!(
            night.contains(6, 5 * 60),
            "saturday 05:00 belongs to friday's window"
        );
        assert!(
            !night.contains(5, 5 * 60),
            "friday 05:00 belongs to thursday's window"
        );
        assert!(
            TimeWindow {
                days: vec![8],
                start: "1:00".into(),
                end: "2:00".into()
            }
            .validate()
            .is_err()
        );
        assert!(
            TimeWindow {
                days: vec![],
                start: "25:00".into(),
                end: "2:00".into()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn engine_defaults_fill_gaps() {
        let mut s = State::default();
        s.settings
            .insert("engine".into(), json!({"max_watch_slots": 1}));
        let e = EngineSettings::from_state(&s);
        assert_eq!(e.max_watch_slots, 1);
        assert!(e.manifest_auto_update);
    }
}

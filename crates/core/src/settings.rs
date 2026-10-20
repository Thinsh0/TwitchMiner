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
        }
    }
}

impl EngineSettings {
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
    fn engine_defaults_fill_gaps() {
        let mut s = State::default();
        s.settings
            .insert("engine".into(), json!({"max_watch_slots": 1}));
        let e = EngineSettings::from_state(&s);
        assert_eq!(e.max_watch_slots, 1);
        assert!(e.manifest_auto_update);
    }
}

//! Two access levels: `Full` (the owner, the apps) and `Agent` (AI agents).
//!
//! An agent reads text written by strangers (stream titles, campaign names…) and can
//! be manipulated by it. Its token therefore cannot delete accounts, redirect traffic
//! (proxy, peers, update URLs, notification targets) or touch files, and never sees
//! secrets (peer tokens, webhook URLs, bot tokens, proxy credentials).

use serde_json::Value;
use tm_core::{ApiError, Command, Event};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Full,
    Agent,
}

/// Engine settings an agent may change.
const AGENT_ENGINE_KEYS: &[&str] = &["paused", "active_hours", "max_watch_slots"];

fn denied(what: &str) -> ApiError {
    ApiError::new("forbidden", format!("the agent token cannot {what}"))
}

fn is_notify_scope(scope: &str) -> bool {
    scope == "module:notify" || scope.ends_with("/module:notify")
}

pub fn authorize(access: Access, cmd: &Command) -> Result<(), ApiError> {
    if access == Access::Full {
        return Ok(());
    }
    match cmd {
        Command::AccountRemove { .. } => Err(denied("remove accounts")),
        Command::LoginWithToken { .. } => Err(denied("add accounts from raw tokens")),
        Command::Backup { .. } => Err(denied("write backups")),
        Command::SettingsSet { scope, value, .. } => {
            if is_notify_scope(scope) {
                return Err(denied("change notification targets"));
            }
            if scope == tm_core::settings::ENGINE_SCOPE {
                let ok = value
                    .as_object()
                    .is_some_and(|o| o.keys().all(|k| AGENT_ENGINE_KEYS.contains(&k.as_str())));
                if !ok {
                    return Err(denied(&format!(
                        "change engine settings other than {}",
                        AGENT_ENGINE_KEYS.join(", ")
                    )));
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

const HIDDEN: &str = "***";

fn hide(v: &mut Value) {
    if !v.is_null() {
        *v = Value::String(HIDDEN.into());
    }
}

/// Masks secrets inside one settings document.
pub fn redact_settings(scope: &str, doc: &mut Value) {
    if scope == tm_core::settings::ENGINE_SCOPE {
        hide(&mut doc["proxy"]);
        if let Some(peers) = doc.get_mut("peers").and_then(Value::as_array_mut) {
            for p in peers {
                if let Some(t) = p.get_mut("token") {
                    hide(t);
                }
            }
        }
    }
    if is_notify_scope(scope)
        && let Some(targets) = doc.get_mut("targets").and_then(Value::as_array_mut)
    {
        for t in targets {
            for k in [
                "webhook_url",
                "url",
                "token",
                "bot_token",
                "chat_id",
                "headers",
            ] {
                if let Some(v) = t.get_mut(k) {
                    hide(v);
                }
            }
        }
    }
}

/// Masks secrets in a serialized `State`.
pub fn redact_state(state: &mut Value) {
    if let Some(settings) = state.get_mut("settings").and_then(Value::as_object_mut) {
        for (scope, doc) in settings.iter_mut() {
            redact_settings(scope, doc);
        }
    }
}

/// Masks secrets in a serialized `Envelope` (only `settings_changed` carries any).
pub fn redact_envelope(env: &Event, json: &mut Value) {
    if let Event::SettingsChanged { scope, .. } = env {
        redact_settings(scope, &mut json["value"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn set(scope: &str, value: Value) -> Command {
        Command::SettingsSet {
            scope: scope.into(),
            value,
            merge: true,
        }
    }

    #[test]
    fn agent_limits() {
        let a = Access::Agent;
        assert!(authorize(a, &set("engine", json!({"paused": true}))).is_ok());
        assert!(authorize(a, &set("engine", json!({"proxy": "http://evil"}))).is_err());
        assert!(authorize(a, &set("engine", json!({"paused": true, "peers": []}))).is_err());
        assert!(authorize(a, &set("engine", Value::Null)).is_err());
        assert!(authorize(a, &set("module:drops", json!({"games": ["Rust"]}))).is_ok());
        assert!(authorize(a, &set("account:1/module:notify", json!({"targets": []}))).is_err());
        assert!(
            authorize(
                a,
                &Command::AccountRemove {
                    account: "1".into()
                }
            )
            .is_err()
        );
        assert!(authorize(a, &Command::Snapshot).is_ok());
        assert!(authorize(a, &set("engine", json!({"agent_enabled": true}))).is_err());
        assert!(
            authorize(
                Access::Full,
                &Command::AccountRemove {
                    account: "1".into()
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn secrets_are_masked() {
        let mut state = json!({"settings": {
            "engine": {"proxy": "http://u:p@h:1", "peers": [{"url": "http://x", "token": "s3cret"}], "paused": false},
            "module:notify": {"targets": [{"kind": "discord", "webhook_url": "https://discord/hook"}]},
            "module:drops": {"games": ["Rust"]}
        }});
        redact_state(&mut state);
        let s = state.to_string();
        assert!(!s.contains("s3cret") && !s.contains("discord/hook") && !s.contains("u:p@"));
        assert!(s.contains("Rust") && s.contains("http://x"));
    }
}

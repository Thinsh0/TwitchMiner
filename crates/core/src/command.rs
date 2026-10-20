//! The command API: everything a client can ask the engine to do.
//!
//! Commands and responses are plain JSON-serializable values so the exact same
//! API works in-process (`Engine::execute`) and over the network (`tm-server`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::{HistoryQuery, StatsQuery};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Full state (the same value the event stream keeps up to date).
    Snapshot,
    /// Registered modules with their default settings.
    ModulesList,

    /// Starts a device-code login. Returns the code to show; completion arrives as
    /// a `login_finished` event.
    LoginStart {
        #[serde(default)]
        profile: Option<String>,
    },
    LoginCancel {
        flow_id: String,
    },
    /// Adds an account from an existing OAuth token (advanced / scripting).
    LoginWithToken {
        access_token: String,
        #[serde(default)]
        refresh_token: Option<String>,
        #[serde(default)]
        profile: Option<String>,
    },
    AccountRemove {
        account: String,
    },
    AccountSetEnabled {
        account: String,
        enabled: bool,
    },

    /// Returns `{ raw, effective, defaults }` for a scope. `account` is needed to
    /// compute the effective value of a `module:<id>` scope for one account.
    SettingsGet {
        scope: String,
        #[serde(default)]
        account: Option<String>,
    },
    /// Replaces (or deep-merges with `merge: true`) a settings document.
    /// `value: null` deletes the scope.
    SettingsSet {
        scope: String,
        value: Value,
        #[serde(default)]
        merge: bool,
    },

    History {
        #[serde(default, flatten)]
        query: HistoryQuery,
    },
    Stats {
        #[serde(default, flatten)]
        query: StatsQuery,
    },

    /// Calls a module-defined action (e.g. `drops.refresh`, `points.add_channel`).
    ModuleCall {
        account: String,
        module: String,
        name: String,
        #[serde(default)]
        args: Value,
    },

    /// Checks for a newer Twitch manifest now.
    ManifestRefresh,
    NoticeDismiss {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
    pub fn not_found(what: impl Into<String>) -> Self {
        Self::new("not_found", what)
    }
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::new("invalid", msg)
    }
    pub fn internal(msg: impl std::fmt::Display) -> Self {
        Self::new("internal", msg.to_string())
    }
}

pub type ApiResult = Result<Value, ApiError>;

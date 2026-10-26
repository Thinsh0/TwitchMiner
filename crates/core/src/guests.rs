//! Guests: people the owner lets use this engine for their own Twitch accounts only
//! (a friend on the owner's server). Each has a token; `tm-server` shows them only
//! their accounts and lets them change only those.
//!
//! Stored in the `guests` settings document, `{ "<id>": Guest }`, which keeps the
//! SHA-256 of each token, never the token itself.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::state::State;

pub const GUESTS_SCOPE: &str = "guests";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Guest {
    pub name: String,
    /// Lowercase hex SHA-256 of the guest's token.
    pub token_sha256: String,
    /// Twitch accounts (ids) this guest added: the only ones they see and change.
    #[serde(default)]
    pub accounts: Vec<String>,
    pub created_at: DateTime<Utc>,
}

/// Every guest, by id (an unreadable document counts as none).
pub fn all(state: &State) -> BTreeMap<String, Guest> {
    state
        .settings
        .get(GUESTS_SCOPE)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// The guest holding `token`, if any.
pub fn by_token(state: &State, token: &str) -> Option<(String, Guest)> {
    if token.is_empty() {
        return None;
    }
    let hash = hash(token);
    all(state)
        .into_iter()
        .find(|(_, g)| constant_time_eq(g.token_sha256.as_bytes(), hash.as_bytes()))
}

/// The guest an account belongs to.
pub fn owner_of(state: &State, account: &str) -> Option<String> {
    all(state)
        .into_iter()
        .find(|(_, g)| g.accounts.iter().any(|a| a == account))
        .map(|(id, _)| id)
}

pub fn hash(token: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, token.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A new random token (256 bits, hex).
pub fn new_token() -> String {
    (0..4)
        .map(|_| format!("{:016x}", rand::random::<u64>()))
        .collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tokens_are_found_by_hash_only() {
        let token = new_token();
        assert_eq!(token.len(), 64);
        let mut state = State::default();
        state.settings.insert(
            GUESTS_SCOPE.into(),
            json!({"g1": {"name": "Sam", "token_sha256": hash(&token), "accounts": ["42"],
                          "created_at": "2026-10-26T00:00:00Z"}}),
        );
        let (id, g) = by_token(&state, &token).unwrap();
        assert_eq!((id.as_str(), g.name.as_str()), ("g1", "Sam"));
        assert!(by_token(&state, "wrong").is_none());
        assert!(by_token(&state, "").is_none());
        assert_eq!(owner_of(&state, "42").as_deref(), Some("g1"));
        assert_eq!(owner_of(&state, "7"), None);
        assert!(!state.settings[GUESTS_SCOPE].to_string().contains(&token));
    }
}

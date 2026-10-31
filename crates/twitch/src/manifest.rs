//! The Twitch "manifest": every volatile identifier Twitch can change under our feet
//! (persisted-query hashes, client IDs, user agents, spade URL).
//!
//! A copy is embedded at build time; a newer revision can be fetched at runtime
//! (see [`Manifest::fetch`]) so that a hash change on Twitch's side is fixed by
//! publishing a JSON file instead of shipping a new build.

use std::collections::BTreeMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

pub const EMBEDDED: &str = include_str!("../../../manifest/twitch.json");

/// Highest manifest schema understood by this build.
pub const SUPPORTED_SCHEMA: u32 = 1;

/// A manifest more than this many revisions ahead of the current one is refused: a
/// `revision` near `u32::MAX` would otherwise block every later fix.
pub const MAX_REVISION_JUMP: u32 = 1000;

/// A downloaded manifest larger than this is refused (the real one is a few KB).
pub const MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub revision: u32,
    #[serde(default)]
    pub updated: String,
    pub default_profile: String,
    pub profiles: BTreeMap<String, ClientProfile>,
    pub web_user_agent: String,
    pub spade_url: String,
    /// Drop campaigns of these "games" (e.g. Special Events) count on any allowed
    /// channel, whatever it streams.
    #[serde(default)]
    pub special_game_ids: Vec<String>,
    pub operations: BTreeMap<String, Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientProfile {
    pub client_id: String,
    pub client_url: String,
    pub user_agents: Vec<String>,
    /// Whether this client may send non-persisted (raw) GQL queries.
    #[serde(default)]
    pub raw_queries: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub name: String,
    pub hash: String,
    /// Default variables, deep-merged under the caller's variables.
    #[serde(default)]
    pub variables: Value,
}

impl Manifest {
    pub fn embedded() -> Self {
        let m: Manifest = serde_json::from_str(EMBEDDED).expect("embedded manifest is valid");
        m.validate().expect("embedded manifest is consistent");
        m
    }

    pub fn parse(json: &str) -> Result<Self> {
        let m: Manifest = serde_json::from_str(json)?;
        m.validate()?;
        Ok(m)
    }

    /// Checks the schema and every identifier and address (only Twitch over https).
    pub fn validate(&self) -> Result<()> {
        if self.schema > SUPPORTED_SCHEMA {
            return Err(Error::decode(format!(
                "manifest schema {} unsupported",
                self.schema
            )));
        }
        if !self.profiles.contains_key(&self.default_profile) {
            return Err(Error::decode("default profile missing"));
        }
        for (key, op) in &self.operations {
            if op.hash.len() != 64 || !op.hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::decode(format!("bad hash for {key}")));
            }
        }
        for p in self.profiles.values() {
            if p.client_id.is_empty() || p.user_agents.is_empty() {
                return Err(Error::decode("profile without client id / user agent"));
            }
            if !is_https(&p.client_url) {
                return Err(Error::decode("client_url must be an https:// address"));
            }
        }
        if !is_https(&self.spade_url) || !is_twitch_host(&self.spade_url) {
            return Err(Error::decode(
                "spade_url must be an https:// address on twitch.tv",
            ));
        }
        Ok(())
    }

    /// Whether this manifest may replace `current`: newer, but not absurdly so.
    pub fn may_replace(&self, current: &Manifest) -> bool {
        self.revision > current.revision && self.revision - current.revision <= MAX_REVISION_JUMP
    }

    pub fn profile(&self, name: &str) -> Option<&ClientProfile> {
        self.profiles.get(name)
    }

    pub fn operation(&self, key: &str) -> Result<&Operation> {
        self.operations
            .get(key)
            .ok_or_else(|| Error::UnknownOperation(key.to_string()))
    }

    /// Downloads a manifest and returns it only if it is valid and newer than `current`.
    pub async fn fetch(
        http: &reqwest::Client,
        url: &str,
        current: &Manifest,
    ) -> Result<Option<Self>> {
        let mut resp = http
            .get(url)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await?
            .error_for_status()?;
        if resp.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
            return Err(Error::decode("manifest too large"));
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await? {
            if body.len() + chunk.len() > MAX_BYTES {
                return Err(Error::decode("manifest too large"));
            }
            body.extend_from_slice(&chunk);
        }
        let text = String::from_utf8(body).map_err(|_| Error::decode("manifest is not UTF-8"))?;
        let m = Manifest::parse(&text)?;
        if m.revision > current.revision && !m.may_replace(current) {
            return Err(Error::decode(format!(
                "manifest revision {} is too far ahead of {}",
                m.revision, current.revision
            )));
        }
        Ok(m.may_replace(current).then_some(m))
    }
}

fn is_https(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some())
}

fn is_twitch_host(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| {
        u.host_str()
            .is_some_and(|h| h == "twitch.tv" || h.ends_with(".twitch.tv"))
    })
}

/// Shared, hot-swappable manifest handle.
pub type SharedManifest = Arc<ArcSwap<Manifest>>;

pub fn shared(m: Manifest) -> SharedManifest {
    Arc::new(ArcSwap::from_pointee(m))
}

/// Recursively merges `overlay` into `base` (objects merged, everything else replaced).
pub fn merge_json(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge_json(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, o) => *b = o.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn embedded_is_valid() {
        let m = Manifest::embedded();
        assert!(m.operation("Inventory").is_ok());
        assert!(m.profile(&m.default_profile).is_some());
    }

    #[test]
    fn addresses_and_revisions_are_checked() {
        let base = Manifest::embedded();
        let with = |f: &dyn Fn(&mut Manifest)| {
            let mut m = base.clone();
            f(&mut m);
            m.validate()
        };
        assert!(with(&|m| m.spade_url = "http://spade.twitch.tv/track".into()).is_err());
        assert!(with(&|m| m.spade_url = "https://evil.example/track".into()).is_err());
        assert!(with(&|m| m.spade_url = "https://twitch.tv.evil.example/x".into()).is_err());
        assert!(with(&|m| m.spade_url = "https://video-edge.twitch.tv/x".into()).is_ok());
        assert!(
            with(&|m| {
                for p in m.profiles.values_mut() {
                    p.client_url = "http://www.twitch.tv".into();
                }
            })
            .is_err()
        );
        let mut next = base.clone();
        next.revision = base.revision + 1;
        assert!(next.may_replace(&base));
        assert!(!base.may_replace(&next));
        next.revision = u32::MAX;
        assert!(!next.may_replace(&base));
    }

    #[test]
    fn merge_is_deep() {
        let mut a = json!({"a": 1, "o": {"x": 1, "y": 2}});
        merge_json(&mut a, &json!({"o": {"y": 3, "z": 4}, "b": true}));
        assert_eq!(a, json!({"a": 1, "b": true, "o": {"x": 1, "y": 3, "z": 4}}));
    }
}

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

    fn validate(&self) -> Result<()> {
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
        }
        Ok(())
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
        let text = http
            .get(url)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let m = Manifest::parse(&text)?;
        Ok((m.revision > current.revision).then_some(m))
    }
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
    fn merge_is_deep() {
        let mut a = json!({"a": 1, "o": {"x": 1, "y": 2}});
        merge_json(&mut a, &json!({"o": {"y": 3, "z": 4}, "b": true}));
        assert_eq!(a, json!({"a": 1, "b": true, "o": {"x": 1, "y": 3, "z": 4}}));
    }
}

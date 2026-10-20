//! The module system.
//!
//! A feature = one type implementing [`Module`]. The engine runs one instance of
//! `run` per account, supervises it (restart with backoff on error or panic,
//! start/stop when its `enabled` setting changes) and hands it a [`ModuleContext`]
//! with everything it may touch.
//!
//! ```ignore
//! struct Hello;
//!
//! #[async_trait::async_trait]
//! impl Module for Hello {
//!     fn info(&self) -> ModuleInfo {
//!         ModuleInfo::new("hello", "Hello", "Logs a greeting", serde_json::json!({"enabled": true}))
//!     }
//!     async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
//!         tracing::info!(account = %ctx.account_id(), "hello");
//!         ctx.cancelled().await;
//!         Ok(())
//!     }
//! }
//! ```

use std::sync::Arc;
use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tm_twitch::{Gql, PubSub};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::db::Db;
use crate::event::{Envelope, Event};
use crate::scheduler::{WatchHandle, WatchRequest};
use crate::settings::module_effective;
use crate::state::ChannelMeta;
use crate::store::Store;

#[derive(Debug, Clone, Serialize)]
pub struct ModuleInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Default settings. Must contain `"enabled": bool`.
    pub default_settings: Value,
    /// JSON Schema of the settings document (lets UIs build forms). `null` if unknown.
    pub settings_schema: Value,
}

impl ModuleInfo {
    pub fn new(
        id: &'static str,
        name: &'static str,
        description: &'static str,
        default_settings: Value,
    ) -> Self {
        let mut default_settings = default_settings;
        if default_settings.get("enabled").is_none() {
            default_settings["enabled"] = json!(true);
        }
        Self {
            id,
            name,
            description,
            default_settings,
            settings_schema: Value::Null,
        }
    }

    /// Attaches the JSON Schema of the settings type `T` (doc comments become descriptions).
    pub fn with_schema<T: schemars::JsonSchema>(mut self) -> Self {
        self.settings_schema = serde_json::to_value(schemars::schema_for!(T)).unwrap_or_default();
        self
    }
}

#[async_trait::async_trait]
pub trait Module: Send + Sync + 'static {
    fn info(&self) -> ModuleInfo;

    /// Checks an effective settings document before it is saved.
    fn validate_settings(&self, _settings: &Value) -> Result<(), String> {
        Ok(())
    }

    /// Runs the module for one account until `ctx` is cancelled.
    /// Returning an error triggers a restart with backoff.
    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()>;
}

/// Helper for modules with a typed settings struct: validates by deserializing.
pub fn validate_as<T: DeserializeOwned>(v: &Value) -> Result<(), String> {
    serde_json::from_value::<T>(v.clone())
        .map(drop)
        .map_err(|e| e.to_string())
}

/// A request sent to a running module (`Command::ModuleCall`).
pub struct ModuleCall {
    pub name: String,
    pub args: Value,
    pub reply: oneshot::Sender<Result<Value, String>>,
}

/// Account-scoped services shared by every module of one account.
#[derive(Clone)]
pub struct AccountServices {
    pub account_id: String,
    pub login: String,
    pub gql: Gql,
    pub pubsub: PubSub,
    pub store: Store,
    pub watch: WatchHandle,
    pub settings_gen: watch::Receiver<u64>,
}

pub struct ModuleContext {
    pub(crate) services: AccountServices,
    pub(crate) module_id: &'static str,
    pub(crate) defaults: Value,
    pub(crate) settings_rx: tokio::sync::Mutex<watch::Receiver<u64>>,
    pub(crate) calls: tokio::sync::Mutex<mpsc::Receiver<ModuleCall>>,
    pub(crate) cancel: CancellationToken,
}

impl ModuleContext {
    pub fn module_id(&self) -> &'static str {
        self.module_id
    }

    pub fn account_id(&self) -> &str {
        &self.services.account_id
    }

    /// The account's own login.
    pub fn login(&self) -> &str {
        &self.services.login
    }

    pub fn gql(&self) -> &Gql {
        &self.services.gql
    }

    pub fn pubsub(&self) -> &PubSub {
        &self.services.pubsub
    }

    pub fn store(&self) -> &Store {
        &self.services.store
    }

    pub fn db(&self) -> &Db {
        self.services.store.db()
    }

    pub fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    pub async fn cancelled(&self) {
        self.cancel.cancelled().await
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Sleeps; returns `false` if the module was cancelled meanwhile.
    pub async fn sleep(&self, d: Duration) -> bool {
        tokio::select! {
            _ = self.cancel.cancelled() => false,
            _ = tokio::time::sleep(d) => true,
        }
    }

    // ------------------------------------------------------------ settings

    pub fn settings_value(&self) -> Value {
        self.services.store.read(|s| {
            module_effective(s, &self.services.account_id, self.module_id, &self.defaults)
        })
    }

    pub fn settings<T: DeserializeOwned>(&self) -> anyhow::Result<T> {
        Ok(serde_json::from_value(self.settings_value())?)
    }

    /// Resolves when any settings document relevant to this account changed
    /// (may fire spuriously: compare before acting).
    pub async fn settings_changed(&self) {
        let mut rx = self.settings_rx.lock().await;
        if rx.changed().await.is_err() {
            // Sender gone: the account is shutting down.
            drop(rx);
            self.cancel.cancelled().await;
        }
    }

    // -------------------------------------------------------------- events

    pub fn emit(&self, event: Event) -> Option<u64> {
        self.services.store.emit(event)
    }

    pub fn events(&self) -> broadcast::Receiver<Arc<Envelope>> {
        self.services.store.subscribe()
    }

    /// Publishes module-owned data in the state (`accounts.<id>.module_data.<module>.<key>`).
    pub fn set_data(&self, key: &str, value: Value) {
        self.emit(Event::ModuleDataSet {
            account: self.services.account_id.clone(),
            module: self.module_id.to_string(),
            key: key.to_string(),
            value,
        });
    }

    pub fn track_channel(&self, channel: ChannelMeta) {
        self.emit(Event::ChannelTracked {
            account: self.services.account_id.clone(),
            channel,
            source: self.module_id.to_string(),
        });
    }

    pub fn release_channel(&self, channel_id: &str) {
        self.emit(Event::ChannelReleased {
            account: self.services.account_id.clone(),
            channel_id: channel_id.to_string(),
            source: self.module_id.to_string(),
        });
    }

    // ------------------------------------------------------------ watching

    /// Replaces this module's watch requests. The scheduler picks the
    /// highest-priority channels across all modules.
    pub fn set_watch_requests(&self, requests: Vec<WatchRequest>) {
        self.services.watch.set(self.module_id, requests);
    }

    // --------------------------------------------------------------- calls

    /// Next request addressed to this module (`None` once the engine dropped it).
    pub async fn next_call(&self) -> Option<ModuleCall> {
        self.calls.lock().await.recv().await
    }

    // ------------------------------------------------------------------ kv

    fn kv_ns(&self) -> String {
        format!("{}/{}", self.services.account_id, self.module_id)
    }

    pub async fn kv_get<T: DeserializeOwned>(&self, key: &str) -> anyhow::Result<Option<T>> {
        let v = self.db().kv_get(self.kv_ns(), key.to_string()).await?;
        Ok(v.map(serde_json::from_value).transpose()?)
    }

    pub async fn kv_put<T: Serialize>(&self, key: &str, value: &T) -> anyhow::Result<()> {
        self.db()
            .kv_put(self.kv_ns(), key.to_string(), serde_json::to_value(value)?)
            .await?;
        Ok(())
    }
}

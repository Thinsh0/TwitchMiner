//! The engine: owns the store, the accounts and the modules, and executes commands.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use chrono::Utc;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tm_twitch::auth;
use tm_twitch::manifest::{self, Manifest, SharedManifest, merge_json};
use tm_twitch::{Net, NetConfig};
use tokio_util::sync::CancellationToken;

use crate::account::AccountRuntime;
use crate::command::{ApiError, ApiResult, Command};
use crate::db::{Db, StoredAccount};
use crate::event::Event;
use crate::module::{Module, ModuleCall};
use crate::paths::Paths;
use crate::settings::{self, ENGINE_SCOPE, EngineSettings, Scope};
use crate::state::{AccountInfo, AccountStatus, Level, LoginFlow, Notice};
use crate::store::Store;

pub struct EngineConfig {
    pub paths: Paths,
    pub modules: Vec<Arc<dyn Module>>,
    /// Version string of the host application.
    pub version: String,
}

/// Cheap to clone; all clones drive the same engine.
#[derive(Clone)]
pub struct Engine {
    shared: Arc<Shared>,
}

pub(crate) struct Shared {
    pub paths: Paths,
    pub store: Store,
    pub manifest: SharedManifest,
    pub modules: Vec<Arc<dyn Module>>,
    pub cancel: CancellationToken,
    net: ArcSwap<Net>,
    accounts: tokio::sync::Mutex<HashMap<String, AccountRuntime>>,
    logins: Mutex<HashMap<String, CancellationToken>>,
}

impl Shared {
    pub fn net(&self) -> Net {
        (**self.net.load()).clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("database: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("network setup: {0}")]
    Net(#[from] tm_twitch::Error),
    #[error("duplicate module id `{0}`")]
    DuplicateModule(String),
}

const MANIFEST_KV: (&str, &str) = ("engine", "manifest");

impl Engine {
    pub async fn start(cfg: EngineConfig) -> Result<Self, EngineError> {
        let mut seen = std::collections::HashSet::new();
        for m in &cfg.modules {
            let id = m.info().id;
            if !seen.insert(id) {
                return Err(EngineError::DuplicateModule(id.to_string()));
            }
        }

        let db = Db::open(&cfg.paths.db_file())?;
        let store = Store::new(db.clone());
        store.emit(Event::EngineStarted {
            version: cfg.version.clone(),
        });
        for (scope, value) in db.settings().await? {
            store.emit(Event::SettingsChanged { scope, value });
        }

        let mut m = Manifest::embedded();
        if let Some(cached) = db
            .kv_get(MANIFEST_KV.0.into(), MANIFEST_KV.1.into())
            .await?
        {
            match serde_json::from_value::<Manifest>(cached) {
                Ok(c) if c.revision > m.revision && c.schema <= manifest::SUPPORTED_SCHEMA => m = c,
                _ => {}
            }
        }
        store.emit(Event::ManifestUpdated {
            revision: m.revision,
        });

        let engine_settings = store.read(EngineSettings::from_state);
        let net = Net::new(&NetConfig {
            proxy: engine_settings.proxy.clone(),
        })?;

        let shared = Arc::new(Shared {
            paths: cfg.paths,
            store: store.clone(),
            manifest: manifest::shared(m),
            modules: cfg.modules,
            cancel: CancellationToken::new(),
            net: ArcSwap::from_pointee(net),
            accounts: tokio::sync::Mutex::new(HashMap::new()),
            logins: Mutex::new(HashMap::new()),
        });
        let engine = Engine { shared };

        for acc in db.accounts().await? {
            engine.publish_account(&acc);
            store.emit(Event::AccountStatusChanged {
                account: acc.id.clone(),
                status: AccountStatus::Stopped,
            });
            if acc.enabled {
                engine.start_account(acc).await;
            }
        }

        engine.spawn_background_tasks();
        tracing::info!(data = %engine.shared.paths.root.display(), "engine started");
        Ok(engine)
    }

    pub fn store(&self) -> &Store {
        &self.shared.store
    }

    pub fn paths(&self) -> &Paths {
        &self.shared.paths
    }

    pub fn modules(&self) -> &[Arc<dyn Module>] {
        &self.shared.modules
    }

    pub async fn shutdown(&self) {
        let runtimes: Vec<AccountRuntime> = self
            .shared
            .accounts
            .lock()
            .await
            .drain()
            .map(|(_, r)| r)
            .collect();
        for r in runtimes {
            r.stop().await;
        }
        self.shared.cancel.cancel();
        // Pending writes are queued before this call; then fold the WAL into the file.
        if let Err(e) = self.shared.store.db().checkpoint().await {
            tracing::warn!(error = %e, "database checkpoint failed");
        }
    }

    // ------------------------------------------------------------------ accounts

    fn publish_account(&self, acc: &StoredAccount) {
        self.shared.store.emit(Event::AccountUpserted {
            info: AccountInfo {
                id: acc.id.clone(),
                login: acc.login.clone(),
                display_name: acc.display_name.clone(),
                avatar_url: acc.avatar_url.clone(),
                enabled: acc.enabled,
            },
        });
    }

    async fn start_account(&self, acc: StoredAccount) {
        let id = acc.id.clone();
        let mut map = self.shared.accounts.lock().await;
        if let Some(old) = map.remove(&id) {
            old.stop().await;
        }
        map.insert(id, AccountRuntime::start(self.shared.clone(), acc));
    }

    async fn stop_account(&self, id: &str) {
        let rt = self.shared.accounts.lock().await.remove(id);
        if let Some(rt) = rt {
            rt.stop().await;
        }
    }

    async fn restart_all_accounts(&self) {
        let Ok(accounts) = self.shared.store.db().accounts().await else {
            return;
        };
        for acc in accounts.into_iter().filter(|a| a.enabled) {
            self.start_account(acc).await;
        }
    }

    async fn add_account(&self, creds: auth::Credentials) -> Result<String, ApiError> {
        let db = self.shared.store.db();
        let existing = db.accounts().await.map_err(ApiError::internal)?;
        let prev = existing.iter().find(|a| a.id == creds.user_id);
        let acc = StoredAccount {
            id: creds.user_id.clone(),
            login: creds.login.clone(),
            display_name: prev
                .map(|p| p.display_name.clone())
                .unwrap_or_else(|| creds.login.clone()),
            avatar_url: prev.and_then(|p| p.avatar_url.clone()),
            enabled: true,
            credentials: creds,
        };
        db.upsert_account(acc.clone())
            .await
            .map_err(ApiError::internal)?;
        self.publish_account(&acc);
        let id = acc.id.clone();
        self.start_account(acc).await;
        Ok(id)
    }

    // ----------------------------------------------------------- background

    fn spawn_background_tasks(&self) {
        // Manifest and application updates.
        let me = self.clone();
        tokio::spawn(async move {
            let cancel = me.shared.cancel.clone();
            let mut delay = Duration::from_secs(30);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(delay) => {}
                }
                delay = Duration::from_secs(6 * 60 * 60);
                let settings = me.shared.store.read(EngineSettings::from_state);
                if settings.manifest_auto_update
                    && let Err(e) = me.refresh_manifest().await
                {
                    tracing::debug!(error = %e.message, "manifest check failed");
                }
                if settings.update_check
                    && let Err(e) = me.check_update().await
                {
                    tracing::debug!(error = %e.message, "update check failed");
                }
            }
        });

        // Proxy changes need a new network stack; restart accounts on it.
        let me = self.clone();
        tokio::spawn(async move {
            let mut rx = me.shared.store.subscribe();
            let mut proxy = me.shared.store.read(EngineSettings::from_state).proxy;
            loop {
                let env = tokio::select! {
                    _ = me.shared.cancel.cancelled() => return,
                    r = rx.recv() => match r { Ok(e) => e, Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue, Err(_) => return },
                };
                let Event::SettingsChanged { scope, .. } = &env.event else {
                    continue;
                };
                if scope != ENGINE_SCOPE {
                    continue;
                }
                let next = me.shared.store.read(EngineSettings::from_state).proxy;
                if next == proxy {
                    continue;
                }
                proxy = next;
                match Net::new(&NetConfig {
                    proxy: proxy.clone(),
                }) {
                    Ok(net) => {
                        me.shared.net.store(Arc::new(net));
                        tracing::info!("proxy changed, restarting accounts");
                        me.restart_all_accounts().await;
                        me.clear_notice("proxy");
                    }
                    Err(e) => me.notice("proxy", Level::Error, format!("Invalid proxy: {e}")),
                }
            }
        });

        // History retention.
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = me.shared.cancel.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                }
                let days = me
                    .shared
                    .store
                    .read(EngineSettings::from_state)
                    .history_retention_days
                    .max(1);
                match me.shared.store.db().prune_history(days).await {
                    Ok(n) if n > 0 => tracing::info!(rows = n, "pruned history"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "history pruning failed"),
                }
                tokio::select! {
                    _ = me.shared.cancel.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(24 * 60 * 60)) => {}
                }
            }
        });
    }

    async fn refresh_manifest(&self) -> Result<Value, ApiError> {
        let Some(url) = self
            .shared
            .store
            .read(EngineSettings::from_state)
            .manifest_url
        else {
            return Ok(json!({ "updated": false, "reason": "no manifest url" }));
        };
        let current = self.shared.manifest.load_full();
        let net = self.shared.net();
        match Manifest::fetch(&net.http, &url, &current).await {
            Ok(Some(m)) => {
                let rev = m.revision;
                let v = serde_json::to_value(&m).map_err(ApiError::internal)?;
                self.shared
                    .store
                    .db()
                    .kv_put(MANIFEST_KV.0.into(), MANIFEST_KV.1.into(), v)
                    .await
                    .map_err(ApiError::internal)?;
                self.shared.manifest.store(Arc::new(m));
                self.shared
                    .store
                    .emit(Event::ManifestUpdated { revision: rev });
                tracing::info!(revision = rev, "manifest updated");
                Ok(json!({ "updated": true, "revision": rev }))
            }
            Ok(None) => Ok(json!({ "updated": false, "revision": current.revision })),
            Err(e) => Err(ApiError::new("manifest", e.to_string())),
        }
    }

    /// Compares the latest GitHub release with the running version and raises an
    /// `update` notice when a newer one exists.
    async fn check_update(&self) -> ApiResult {
        let (url, current) = self.shared.store.read(|s| {
            (
                EngineSettings::from_state(s).releases_url,
                s.version.clone(),
            )
        });
        let net = self.shared.net();
        let release = net
            .http
            .get(&url)
            .header("User-Agent", "TwitchMiner")
            .header("Accept", "application/vnd.github+json")
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| ApiError::new("update", e.to_string()))?;
        if release.status().as_u16() == 404 {
            // No published release yet (or private repository).
            return Ok(json!({ "current": current, "latest": null, "update_available": false }));
        }
        let release: Value = release
            .error_for_status()
            .map_err(|e| ApiError::new("update", e.to_string()))?
            .json()
            .await
            .map_err(|e| ApiError::new("update", e.to_string()))?;
        let tag = release["tag_name"].as_str().unwrap_or_default().to_string();
        let page = release["html_url"].as_str().unwrap_or_default().to_string();
        let newer = is_newer(&tag, &current);
        if newer {
            self.notice(
                "update",
                Level::Info,
                format!("TwitchMiner {tag} is available: {page}"),
            );
        } else {
            self.clear_notice("update");
        }
        Ok(json!({ "current": current, "latest": tag, "update_available": newer, "url": page }))
    }

    pub fn notice(&self, id: &str, level: Level, message: impl Into<String>) {
        self.shared.store.emit(Event::NoticeRaised {
            notice: Notice {
                id: id.to_string(),
                level,
                message: message.into(),
                at: Utc::now(),
            },
        });
    }

    pub fn clear_notice(&self, id: &str) {
        self.shared
            .store
            .emit(Event::NoticeCleared { id: id.to_string() });
    }

    // ------------------------------------------------------------- commands

    pub async fn execute(&self, cmd: Command) -> ApiResult {
        let store = &self.shared.store;
        let db = store.db();
        match cmd {
            Command::Snapshot => serde_json::to_value(store.snapshot()).map_err(ApiError::internal),
            Command::Summary => {
                serde_json::to_value(store.read(|s| crate::view::summary(s, Utc::now())))
                    .map_err(ApiError::internal)
            }
            Command::ModulesList => Ok(Value::Array(
                self.shared
                    .modules
                    .iter()
                    .map(|m| json!(m.info()))
                    .collect(),
            )),

            Command::LoginStart { profile } => self.login_start(profile).await,
            Command::LoginCancel { flow_id } => {
                let token = self.shared.logins.lock().remove(&flow_id);
                match token {
                    Some(t) => {
                        t.cancel();
                        Ok(json!({ "cancelled": true }))
                    }
                    None => Err(ApiError::not_found("login flow")),
                }
            }
            Command::LoginWithToken {
                access_token,
                refresh_token,
                profile,
            } => {
                let profile =
                    profile.unwrap_or_else(|| self.shared.manifest.load().default_profile.clone());
                let net = self.shared.net();
                let (creds, _) = auth::credentials_from_token(
                    &net,
                    &profile,
                    auth::TokenResponse {
                        access_token,
                        refresh_token,
                        expires_in: None,
                    },
                    None,
                )
                .await
                .map_err(|e| ApiError::new("auth", e.to_string()))?;
                let id = self.add_account(creds).await?;
                Ok(json!({ "account": id }))
            }
            Command::AccountRemove { account } => {
                self.stop_account(&account).await;
                db.delete_account(account.clone())
                    .await
                    .map_err(ApiError::internal)?;
                store.emit(Event::AccountRemoved { account });
                Ok(json!({ "removed": true }))
            }
            Command::AccountSetEnabled { account, enabled } => {
                let mut acc = db
                    .accounts()
                    .await
                    .map_err(ApiError::internal)?
                    .into_iter()
                    .find(|a| a.id == account)
                    .ok_or_else(|| ApiError::not_found("account"))?;
                acc.enabled = enabled;
                db.upsert_account(acc.clone())
                    .await
                    .map_err(ApiError::internal)?;
                self.publish_account(&acc);
                if enabled {
                    self.start_account(acc).await;
                } else {
                    self.stop_account(&account).await;
                    store.emit(Event::AccountStatusChanged {
                        account,
                        status: AccountStatus::Stopped,
                    });
                }
                Ok(json!({ "enabled": enabled }))
            }

            Command::SettingsGet { scope, account } => {
                let raw = store
                    .read(|s| s.settings.get(&scope).cloned())
                    .unwrap_or(Value::Null);
                let (defaults, effective, schema) = match settings::parse_scope(&scope) {
                    Some(Scope::Engine) => (
                        json!(EngineSettings::default()),
                        json!(store.read(EngineSettings::from_state)),
                        json!(schemars::schema_for!(EngineSettings)),
                    ),
                    Some(Scope::Module(m)) => {
                        let info = self.module_info(m)?;
                        let d = info.default_settings;
                        let eff = match &account {
                            Some(a) => store.read(|s| settings::module_effective(s, a, m, &d)),
                            None => {
                                let mut v = d.clone();
                                merge_json(&mut v, &raw);
                                v
                            }
                        };
                        (d, eff, info.settings_schema)
                    }
                    Some(Scope::AccountModule { account, module }) => {
                        let info = self.module_info(module)?;
                        let d = info.default_settings;
                        let eff =
                            store.read(|s| settings::module_effective(s, account, module, &d));
                        (d, eff, info.settings_schema)
                    }
                    None => return Err(ApiError::invalid("unknown scope")),
                };
                Ok(json!({
                    "scope": scope, "raw": raw, "effective": effective,
                    "defaults": defaults, "schema": schema,
                }))
            }
            Command::SettingsSet {
                scope,
                value,
                merge,
            } => self.settings_set(scope, value, merge).await,

            Command::History { query } => {
                Ok(json!(db.history(query).await.map_err(ApiError::internal)?))
            }
            Command::Stats { query } => {
                Ok(json!(db.stats(query).await.map_err(ApiError::internal)?))
            }

            Command::ModuleCall {
                account,
                module,
                name,
                args,
            } => {
                let tx = {
                    let map = self.shared.accounts.lock().await;
                    let rt = map
                        .get(&account)
                        .ok_or_else(|| ApiError::not_found("running account"))?;
                    rt.calls.lock().get(&module).cloned()
                }
                .ok_or_else(|| ApiError::not_found("running module"))?;
                let (reply, rx) = tokio::sync::oneshot::channel();
                tx.send(ModuleCall { name, args, reply })
                    .await
                    .map_err(|_| ApiError::new("unavailable", "module stopped"))?;
                match tokio::time::timeout(Duration::from_secs(120), rx).await {
                    Ok(Ok(Ok(v))) => Ok(v),
                    Ok(Ok(Err(e))) => Err(ApiError::new("module", e)),
                    Ok(Err(_)) => Err(ApiError::new("unavailable", "module dropped the call")),
                    Err(_) => Err(ApiError::new("timeout", "module did not answer")),
                }
            }

            Command::ManifestRefresh => self.refresh_manifest().await,
            Command::UpdateCheck => self.check_update().await,
            Command::Backup { path } => {
                let path = match path {
                    Some(p) => std::path::PathBuf::from(p),
                    None => {
                        let dir = self.shared.paths.root.join("backups");
                        std::fs::create_dir_all(&dir).map_err(ApiError::internal)?;
                        dir.join(format!(
                            "twitchminer-{}.db",
                            Utc::now().format("%Y%m%d-%H%M%S")
                        ))
                    }
                };
                db.backup_to(path.clone())
                    .await
                    .map_err(|e| ApiError::new("backup", e.to_string()))?;
                Ok(json!({ "path": path.display().to_string() }))
            }
            Command::NoticeDismiss { id } => {
                self.clear_notice(&id);
                Ok(json!({}))
            }
        }
    }

    fn module_info(&self, id: &str) -> Result<crate::module::ModuleInfo, ApiError> {
        self.shared
            .modules
            .iter()
            .map(|m| m.info())
            .find(|i| i.id == id)
            .ok_or_else(|| ApiError::not_found(format!("module `{id}`")))
    }

    async fn settings_set(&self, scope: String, value: Value, merge: bool) -> ApiResult {
        let store = &self.shared.store;
        let parsed =
            settings::parse_scope(&scope).ok_or_else(|| ApiError::invalid("unknown scope"))?;
        let mut next = value;
        if merge && !next.is_null() {
            let mut base = store
                .read(|s| s.settings.get(&scope).cloned())
                .unwrap_or_else(|| json!({}));
            merge_json(&mut base, &next);
            next = base;
        }
        if !next.is_null() && !next.is_object() {
            return Err(ApiError::invalid("settings must be a JSON object"));
        }

        // Validate the resulting effective document (deleting a scope needs no validation).
        match parsed {
            _ if next.is_null() => {}
            Scope::Engine => {
                let mut eff = json!(EngineSettings::default());
                merge_json(&mut eff, &next);
                serde_json::from_value::<EngineSettings>(eff)
                    .map_err(|e| ApiError::invalid(e.to_string()))?
                    .validate()
                    .map_err(ApiError::invalid)?;
            }
            Scope::Module(module) | Scope::AccountModule { module, .. } => {
                let m = self
                    .shared
                    .modules
                    .iter()
                    .find(|m| m.info().id == module)
                    .ok_or_else(|| ApiError::not_found(format!("module `{module}`")))?;
                let mut eff = m.info().default_settings;
                if let Scope::AccountModule { .. } = parsed
                    && let Some(shared) =
                        store.read(|s| s.settings.get(&settings::module_scope(module)).cloned())
                {
                    merge_json(&mut eff, &shared);
                }
                merge_json(&mut eff, &next);
                m.validate_settings(&eff).map_err(ApiError::invalid)?;
            }
        }

        store
            .db()
            .put_settings(scope.clone(), next.clone())
            .await
            .map_err(ApiError::internal)?;
        store.emit(Event::SettingsChanged {
            scope,
            value: next.clone(),
        });
        Ok(next)
    }

    async fn login_start(&self, profile: Option<String>) -> ApiResult {
        let manifest = self.shared.manifest.load_full();
        let profile_name = profile.unwrap_or_else(|| manifest.default_profile.clone());
        let profile = manifest
            .profile(&profile_name)
            .cloned()
            .ok_or_else(|| ApiError::not_found("client profile"))?;
        let net = self.shared.net();
        let code = auth::start_device_flow(&net, &profile)
            .await
            .map_err(|e| ApiError::new("auth", e.to_string()))?;
        let flow = LoginFlow {
            flow_id: uuid::Uuid::new_v4().to_string(),
            user_code: code.user_code.clone(),
            verification_uri: code.verification_uri.clone(),
            expires_at: code.expires_at,
        };
        let cancel = self.shared.cancel.child_token();
        self.shared
            .logins
            .lock()
            .insert(flow.flow_id.clone(), cancel.clone());
        self.shared
            .store
            .emit(Event::LoginStarted { flow: flow.clone() });

        let me = self.clone();
        let flow_id = flow.flow_id.clone();
        tokio::spawn(async move {
            let result = async {
                let token = auth::wait_device_flow(&net, &profile, &code, &cancel).await?;
                let (creds, _) =
                    auth::credentials_from_token(&net, &profile_name, token, None).await?;
                Ok::<_, tm_twitch::Error>(creds)
            }
            .await;
            me.shared.logins.lock().remove(&flow_id);
            let (account, error) = match result {
                Ok(creds) => match me.add_account(creds).await {
                    Ok(id) => (Some(id), None),
                    Err(e) => (None, Some(e.message)),
                },
                Err(e) => (None, Some(e.to_string())),
            };
            me.shared.store.emit(Event::LoginFinished {
                flow_id,
                account,
                error,
            });
        });
        serde_json::to_value(flow).map_err(ApiError::internal)
    }
}

/// `true` when `latest` (e.g. `v1.2.0`) is a higher version than `current`.
fn is_newer(latest: &str, current: &str) -> bool {
    fn parse(v: &str) -> Option<Vec<u64>> {
        let core = v.trim().trim_start_matches('v').split(['-', '+']).next()?;
        core.split('.').map(|p| p.parse().ok()).collect()
    }
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_compare() {
        assert!(super::is_newer("v0.2.0", "0.1.9"));
        assert!(super::is_newer("1.0.0", "0.9.0-beta"));
        assert!(!super::is_newer("v0.1.0", "0.1.0"));
        assert!(!super::is_newer("garbage", "0.1.0"));
    }
}

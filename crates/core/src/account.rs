//! One running account: token health, PubSub, scheduler and supervised modules.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use parking_lot::Mutex;
use tm_twitch::auth::{self, Credentials};
use tm_twitch::{Gql, PubSub, PubSubEvent};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::db::StoredAccount;
use crate::engine::Shared;
use crate::event::Event;
use crate::module::{AccountServices, Module, ModuleCall, ModuleContext};
use crate::scheduler::{self, WatchHandle};
use crate::settings::{self, Scope};
use crate::state::{AccountInfo, AccountStatus, ModuleHealth};

const TOKEN_CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

pub(crate) struct AccountRuntime {
    pub cancel: CancellationToken,
    pub calls: Arc<Mutex<HashMap<String, mpsc::Sender<ModuleCall>>>>,
    pub task: tokio::task::JoinHandle<()>,
}

impl AccountRuntime {
    pub fn start(shared: Arc<Shared>, account: StoredAccount) -> Self {
        let cancel = shared.cancel.child_token();
        let calls = Arc::new(Mutex::new(HashMap::new()));
        let task = tokio::spawn(run(shared, account, cancel.clone(), calls.clone()));
        Self {
            cancel,
            calls,
            task,
        }
    }

    pub async fn stop(self) {
        self.cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(10), self.task).await;
    }
}

fn set_status(shared: &Shared, account: &str, status: AccountStatus) {
    shared.store.emit(Event::AccountStatusChanged {
        account: account.to_string(),
        status,
    });
}

/// Validates the credentials, refreshing them when possible.
/// `Ok(None)` = the user must log in again.
async fn ensure_valid(
    shared: &Shared,
    creds: &Credentials,
) -> tm_twitch::Result<Option<Credentials>> {
    let net = shared.net();
    match auth::validate(&net, &creds.access_token).await {
        Ok(_) => Ok(Some(creds.clone())),
        Err(tm_twitch::Error::Unauthorized) => {
            let Some(refresh) = &creds.refresh_token else {
                return Ok(None);
            };
            let manifest = shared.manifest.load();
            let Some(profile) = manifest.profile(&creds.profile) else {
                return Ok(None);
            };
            match auth::refresh(&net, profile, refresh).await {
                Ok(t) => {
                    tracing::info!(login = %creds.login, "token refreshed");
                    let mut c = creds.clone();
                    c.access_token = t.access_token;
                    if t.refresh_token.is_some() {
                        c.refresh_token = t.refresh_token;
                    }
                    Ok(Some(c))
                }
                Err(tm_twitch::Error::Unauthorized) => Ok(None),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

async fn run(
    shared: Arc<Shared>,
    mut account: StoredAccount,
    cancel: CancellationToken,
    calls: Arc<Mutex<HashMap<String, mpsc::Sender<ModuleCall>>>>,
) {
    let id = account.id.clone();
    set_status(&shared, &id, AccountStatus::Starting);

    // 1. Make sure the token works (retrying network failures forever).
    let mut backoff =
        tm_twitch::net::Backoff::new(Duration::from_secs(5), Duration::from_secs(300));
    let creds = loop {
        match ensure_valid(&shared, &account.credentials).await {
            Ok(Some(c)) => break c,
            Ok(None) => {
                tracing::warn!(login = %account.login, "token rejected, login required");
                set_status(&shared, &id, AccountStatus::AuthRequired);
                return;
            }
            Err(e) => {
                let delay = backoff.next_delay();
                set_status(
                    &shared,
                    &id,
                    AccountStatus::Error {
                        message: format!("{e} (retrying)"),
                    },
                );
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        }
    };
    if creds != account.credentials {
        account.credentials = creds.clone();
        if let Err(e) = shared.store.db().upsert_account(account.clone()).await {
            tracing::error!(error = %e, "failed to save refreshed token");
        }
    }

    let creds = Arc::new(ArcSwap::from_pointee(creds));
    let gql = Gql::new(shared.net(), shared.manifest.clone(), creds.clone());

    // 2. Refresh profile info shown in the UI.
    if let Ok(mut users) = gql.users_by_id(std::slice::from_ref(&id)).await
        && let Some(Some(u)) = users.pop()
    {
        account.login = u.login.clone();
        account.display_name = u.display_name.clone();
        account.avatar_url = u.avatar_url.clone();
        shared.store.emit(Event::AccountUpserted {
            info: AccountInfo {
                id: id.clone(),
                login: u.login,
                display_name: u.display_name,
                avatar_url: u.avatar_url,
                enabled: account.enabled,
            },
        });
        let _ = shared.store.db().upsert_account(account.clone()).await;
    }

    // 3. Shared services.
    let pubsub = PubSub::new(shared.net(), creds.clone(), cancel.clone());
    let watch_handle = WatchHandle::default();
    let (gen_tx, gen_rx) = watch::channel(0u64);
    spawn_settings_watcher(&shared, &id, gen_tx, cancel.clone());
    tokio::spawn(scheduler::run(
        id.clone(),
        gql.clone(),
        shared.store.clone(),
        watch_handle.clone(),
        gen_rx.clone(),
        cancel.clone(),
    ));

    let services = AccountServices {
        account_id: id.clone(),
        login: account.login.clone(),
        gql: gql.clone(),
        pubsub: pubsub.clone(),
        store: shared.store.clone(),
        watch: watch_handle,
        settings_gen: gen_rx,
    };
    for module in &shared.modules {
        tokio::spawn(supervise(
            module.clone(),
            services.clone(),
            calls.clone(),
            cancel.clone(),
        ));
    }
    set_status(&shared, &id, AccountStatus::Running);

    // 4. Token health: periodic validation + PubSub auth errors.
    let auth_failed = CancellationToken::new();
    {
        let mut rx = pubsub.subscribe();
        let auth_failed = auth_failed.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    ev = rx.recv() => match ev {
                        Ok(PubSubEvent::ListenError { error, .. }) if error == "ERR_BADAUTH" => {
                            auth_failed.cancel();
                            return;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        _ => {}
                    }
                }
            }
        });
    }
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(TOKEN_CHECK_EVERY) => {}
            _ = auth_failed.cancelled() => {}
        }
        match ensure_valid(&shared, &creds.load()).await {
            Ok(Some(c)) => {
                if *creds.load_full() != c {
                    creds.store(Arc::new(c.clone()));
                    account.credentials = c;
                    let _ = shared.store.db().upsert_account(account.clone()).await;
                }
                if auth_failed.is_cancelled() {
                    // Token is fine after all (transient PubSub error); keep running.
                    tracing::warn!("pubsub reported bad auth but token validates");
                    break_on_repeat(&cancel).await;
                }
            }
            Ok(None) => {
                tracing::warn!(login = %account.login, "token revoked");
                cancel.cancel();
                set_status(&shared, &id, AccountStatus::AuthRequired);
                return;
            }
            Err(e) => tracing::debug!(error = %e, "token check failed (network), will retry"),
        }
    }
    set_status(&shared, &id, AccountStatus::Stopped);
}

/// Avoids a hot loop when PubSub keeps reporting bad auth while the token validates.
async fn break_on_repeat(cancel: &CancellationToken) {
    tokio::select! {
        _ = cancel.cancelled() => {}
        _ = tokio::time::sleep(TOKEN_CHECK_EVERY) => {}
    }
}

/// Bumps `gen` whenever a settings scope relevant to `account` changes.
fn spawn_settings_watcher(
    shared: &Shared,
    account: &str,
    generation: watch::Sender<u64>,
    cancel: CancellationToken,
) {
    let mut rx = shared.store.subscribe();
    let account = account.to_string();
    tokio::spawn(async move {
        let mut n = 0u64;
        loop {
            let env = tokio::select! {
                _ = cancel.cancelled() => return,
                r = rx.recv() => match r {
                    Ok(env) => env,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => { n += 1; let _ = generation.send(n); continue; }
                    Err(_) => return,
                },
            };
            let Event::SettingsChanged { scope, .. } = &env.event else {
                continue;
            };
            let relevant = match settings::parse_scope(scope) {
                Some(Scope::Engine | Scope::Module(_)) => true,
                Some(Scope::AccountModule { account: a, .. }) => a == account,
                None => false,
            };
            if relevant {
                n += 1;
                let _ = generation.send(n);
            }
        }
    });
}

fn health(services: &AccountServices, module: &str, health: ModuleHealth) {
    services.store.emit(Event::ModuleHealthChanged {
        account: services.account_id.clone(),
        module: module.to_string(),
        health,
    });
}

fn is_enabled(services: &AccountServices, module: &str, defaults: &serde_json::Value) -> bool {
    services
        .store
        .read(|s| settings::module_effective(s, &services.account_id, module, defaults))
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

/// Runs a module for one account: restart on failure, follow its `enabled` flag.
async fn supervise(
    module: Arc<dyn Module>,
    mut services: AccountServices,
    calls: Arc<Mutex<HashMap<String, mpsc::Sender<ModuleCall>>>>,
    cancel: CancellationToken,
) {
    let info = module.info();
    let id = info.id;
    let mut attempt = 0u32;
    loop {
        if cancel.is_cancelled() {
            return;
        }
        if !is_enabled(&services, id, &info.default_settings) {
            health(&services, id, ModuleHealth::Disabled);
            services.watch.clear(id);
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = services.settings_gen.changed() => continue,
            }
        }

        let run_cancel = cancel.child_token();
        let (call_tx, call_rx) = mpsc::channel(16);
        calls.lock().insert(id.to_string(), call_tx);
        let ctx = ModuleContext {
            services: services.clone(),
            module_id: id,
            defaults: info.default_settings.clone(),
            settings_rx: tokio::sync::Mutex::new(services.settings_gen.clone()),
            calls: tokio::sync::Mutex::new(call_rx),
            cancel: run_cancel.clone(),
        };
        health(&services, id, ModuleHealth::Running);
        let m = module.clone();
        let started = tokio::time::Instant::now();
        let mut task = tokio::spawn(async move { m.run(ctx).await });

        let outcome = loop {
            tokio::select! {
                r = &mut task => break Some(r),
                _ = cancel.cancelled() => break None,
                _ = services.settings_gen.changed() => {
                    if !is_enabled(&services, id, &info.default_settings) {
                        break None;
                    }
                }
            }
        };
        calls.lock().remove(id);
        let outcome = match outcome {
            Some(r) => r,
            None => {
                run_cancel.cancel();
                if tokio::time::timeout(Duration::from_secs(10), &mut task)
                    .await
                    .is_err()
                {
                    task.abort();
                }
                services.watch.clear(id);
                continue;
            }
        };
        services.watch.clear(id);
        let error = match outcome {
            Ok(Ok(())) => {
                if cancel.is_cancelled() {
                    return;
                }
                health(&services, id, ModuleHealth::Stopped);
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = services.settings_gen.changed() => continue,
                }
            }
            Ok(Err(e)) => format!("{e:#}"),
            Err(join) if join.is_panic() => "panicked".to_string(),
            Err(join) => join.to_string(),
        };
        if started.elapsed() > Duration::from_secs(10 * 60) {
            attempt = 0;
        }
        attempt += 1;
        tracing::error!(module = id, account = %services.account_id, attempt, %error, "module failed");
        health(&services, id, ModuleHealth::Restarting { error, attempt });
        let delay = Duration::from_secs(5u64.saturating_mul(1 << attempt.min(6)).min(300));
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

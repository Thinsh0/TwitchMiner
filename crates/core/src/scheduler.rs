//! Watch-slot scheduler: modules declare which channels they would like to watch
//! and with which priority; the scheduler runs watchers for the best N channels
//! (N = `engine.max_watch_slots`, Twitch credits at most 2 at a time).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tm_twitch::Gql;
use tm_twitch::watch::{self, WatchReport, WatchTarget};
use tokio::sync::{Notify, mpsc, watch as tokio_watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::event::Event;
use crate::settings::EngineSettings;
use crate::store::Store;

/// Well-known priorities. Higher wins; modules may use anything in between.
pub mod priority {
    pub const MANUAL: i32 = 1000;
    pub const DROPS: i32 = 500;
    pub const STREAK: i32 = 300;
    pub const POINTS: i32 = 100;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchRequest {
    pub channel_id: String,
    pub login: String,
    pub priority: i32,
    /// Human-readable reason shown in the UI (`drops: Game`, `streak`, …).
    pub reason: String,
}

#[derive(Clone, Default)]
pub struct WatchHandle {
    inner: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    requests: Mutex<BTreeMap<String, Vec<WatchRequest>>>,
    notify: Notify,
}

impl WatchHandle {
    pub fn set(&self, module: &str, requests: Vec<WatchRequest>) {
        let mut map = self.inner.requests.lock();
        let changed = map.get(module) != Some(&requests);
        if requests.is_empty() {
            map.remove(module);
        } else {
            map.insert(module.to_string(), requests);
        }
        drop(map);
        if changed {
            self.inner.notify.notify_one();
        }
    }

    pub fn clear(&self, module: &str) {
        self.set(module, Vec::new());
    }

    fn all(&self) -> Vec<WatchRequest> {
        self.inner
            .requests
            .lock()
            .values()
            .flatten()
            .cloned()
            .collect()
    }
}

/// Picks up to `slots` distinct channels by priority (stable for equal priority),
/// skipping channels in `excluded`.
pub fn select(
    mut requests: Vec<WatchRequest>,
    slots: usize,
    excluded: impl Fn(&str) -> bool,
) -> Vec<WatchRequest> {
    requests.sort_by_key(|r| std::cmp::Reverse(r.priority));
    let mut out: Vec<WatchRequest> = Vec::new();
    for r in requests {
        if out.len() >= slots {
            break;
        }
        if excluded(&r.channel_id) || out.iter().any(|o| o.channel_id == r.channel_id) {
            continue;
        }
        out.push(r);
    }
    out
}

struct Active {
    cancel: CancellationToken,
    errors: u32,
}

const MAX_CONSECUTIVE_ERRORS: u32 = 6;
const OFFLINE_COOLDOWN: Duration = Duration::from_secs(3 * 60);
const ERROR_COOLDOWN: Duration = Duration::from_secs(5 * 60);

pub(crate) async fn run(
    account: String,
    gql: Gql,
    store: Store,
    handle: WatchHandle,
    mut settings_gen: tokio_watch::Receiver<u64>,
    cancel: CancellationToken,
) {
    let mut active: HashMap<String, Active> = HashMap::new();
    let mut cooldown: HashMap<String, Instant> = HashMap::new();
    let (report_tx, mut report_rx) = mpsc::channel::<(String, WatchReport)>(64);
    let mut tick = tokio::time::interval(Duration::from_secs(15));

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = handle.inner.notify.notified() => {}
            _ = settings_gen.changed() => {}
            _ = tick.tick() => {}
            Some((channel_id, report)) = report_rx.recv() => {
                let Some(a) = active.get_mut(&channel_id) else { continue };
                match report {
                    WatchReport::Tick { minute, .. } => {
                        a.errors = 0;
                        if minute {
                            store.emit(Event::WatchMinute { account: account.clone(), channel_id });
                        }
                        continue;
                    }
                    WatchReport::Error(e) => {
                        a.errors += 1;
                        tracing::debug!(%channel_id, error = %e, errors = a.errors, "watch error");
                        if a.errors < MAX_CONSECUTIVE_ERRORS {
                            continue;
                        }
                        stop(&store, &account, &mut active, &channel_id, &format!("error: {e}"));
                        cooldown.insert(channel_id, Instant::now() + ERROR_COOLDOWN);
                    }
                    WatchReport::Offline => {
                        stop(&store, &account, &mut active, &channel_id, "offline");
                        store.emit(Event::StreamChanged { account: account.clone(), channel_id: channel_id.clone(), stream: None });
                        cooldown.insert(channel_id, Instant::now() + OFFLINE_COOLDOWN);
                    }
                }
            }
        }

        // Reconcile.
        let now = Instant::now();
        cooldown.retain(|_, until| *until > now);
        let engine_settings = store.read(EngineSettings::from_state);
        let slots = usize::from(engine_settings.max_watch_slots.clamp(1, 2));
        let allowed = engine_settings.watching_allowed(chrono::Local::now());
        let offline = |id: &str| {
            cooldown.contains_key(id)
                || store.read(|s| {
                    s.accounts
                        .get(&account)
                        .and_then(|a| a.channels.get(id))
                        .is_some_and(|c| c.stream.is_none())
                })
        };
        let desired = if allowed {
            select(handle.all(), slots, offline)
        } else {
            Vec::new()
        };
        let stop_cause = if engine_settings.paused {
            "paused"
        } else {
            "outside active hours"
        };

        let stale: Vec<String> = active
            .keys()
            .filter(|id| !desired.iter().any(|d| &d.channel_id == *id))
            .cloned()
            .collect();
        for id in stale {
            let cause = if allowed { "rescheduled" } else { stop_cause };
            stop(&store, &account, &mut active, &id, cause);
        }
        for req in desired {
            if active.contains_key(&req.channel_id) {
                continue;
            }
            let token = cancel.child_token();
            let (tx, mut rx) = mpsc::channel(8);
            let target = WatchTarget {
                channel_id: req.channel_id.clone(),
                login: req.login.clone(),
            };
            tokio::spawn(watch::run(gql.clone(), target, tx, token.clone()));
            let fwd = report_tx.clone();
            let id = req.channel_id.clone();
            tokio::spawn(async move {
                while let Some(r) = rx.recv().await {
                    if fwd.send((id.clone(), r)).await.is_err() {
                        break;
                    }
                }
            });
            tracing::info!(account = %account, channel = %req.login, reason = %req.reason, "watching");
            store.emit(Event::WatchStarted {
                account: account.clone(),
                channel_id: req.channel_id.clone(),
                reason: req.reason.clone(),
            });
            active.insert(
                req.channel_id,
                Active {
                    cancel: token,
                    errors: 0,
                },
            );
        }
    }

    for (_, a) in active.drain() {
        a.cancel.cancel();
    }
}

fn stop(
    store: &Store,
    account: &str,
    active: &mut HashMap<String, Active>,
    channel_id: &str,
    cause: &str,
) {
    if let Some(a) = active.remove(channel_id) {
        a.cancel.cancel();
        tracing::info!(account, channel_id, cause, "stopped watching");
        store.emit(Event::WatchStopped {
            account: account.to_string(),
            channel_id: channel_id.to_string(),
            cause: cause.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: &str, p: i32) -> WatchRequest {
        WatchRequest {
            channel_id: id.into(),
            login: id.into(),
            priority: p,
            reason: String::new(),
        }
    }

    #[test]
    fn selects_by_priority_and_dedupes() {
        let r = vec![req("a", 100), req("b", 500), req("a", 300), req("c", 100)];
        let s = select(r, 2, |_| false);
        assert_eq!(
            s.iter().map(|r| r.channel_id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(s[1].priority, 300);
    }

    #[test]
    fn skips_excluded() {
        let s = select(vec![req("a", 9), req("b", 1)], 1, |id| id == "a");
        assert_eq!(s[0].channel_id, "b");
    }
}

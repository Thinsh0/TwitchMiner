//! One interface for front-ends, whether the engine runs in-process or elsewhere.
//!
//! ```ignore
//! let backend: Arc<dyn Backend> = match remote_url {
//!     None => Arc::new(Local::new(app.engine.clone())),
//!     Some(url) => Arc::new(Remote::connect(&url, &token).await?),
//! };
//! let mut state = backend.state();           // watch::Receiver<Arc<State>>
//! loop {
//!     state.changed().await?;
//!     render(&state.borrow());
//! }
//! backend.execute(Command::Summary).await?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tm_core::state::apply_envelope;
use tm_core::{ApiError, ApiResult, Command, Engine, Envelope, State};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

/// Connection status of a backend (always `Connected` for a local engine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Connecting,
    Connected,
    Disconnected(String),
}

#[async_trait::async_trait]
pub trait Backend: Send + Sync {
    /// Always-current mirror of the engine state.
    fn state(&self) -> watch::Receiver<Arc<State>>;
    /// Raw event stream (after the mirror applied them), for animations / toasts.
    fn events(&self) -> broadcast::Receiver<Arc<Envelope>>;
    fn link(&self) -> watch::Receiver<Link>;
    async fn execute(&self, cmd: Command) -> ApiResult;
}

/// Request id reserved for internal snapshot resyncs (user ids start at 1).
const RESYNC_ID: u64 = 0;

/// Publishes the mirror at most this often (coalesces bursts of events).
const PUBLISH_EVERY: Duration = Duration::from_millis(50);

// ------------------------------------------------------------------- local

pub struct Local {
    engine: Engine,
    state: watch::Receiver<Arc<State>>,
    events: broadcast::Sender<Arc<Envelope>>,
    link: watch::Receiver<Link>,
    cancel: CancellationToken,
}

impl Local {
    pub fn new(engine: Engine) -> Self {
        let (snapshot, mut rx) = engine.store().snapshot_and_subscribe();
        let (state_tx, state) = watch::channel(Arc::new(snapshot));
        let (events, _) = broadcast::channel(1024);
        let (_link_tx, link) = watch::channel(Link::Connected);
        let cancel = CancellationToken::new();
        let tx = events.clone();
        let store = engine.store().clone();
        let stop = cancel.clone();
        tokio::spawn(async move {
            let mut mirror = (**state_tx.borrow()).clone();
            loop {
                let first = tokio::select! {
                    _ = stop.cancelled() => return,
                    r = rx.recv() => r,
                };
                let mut batch = vec![first];
                tokio::time::sleep(PUBLISH_EVERY).await;
                while let Ok(e) = rx.try_recv() {
                    batch.push(Ok(e));
                }
                for r in batch {
                    match r {
                        Ok(env) => {
                            if env.seq > mirror.seq {
                                apply_envelope(&mut mirror, &env);
                                let _ = tx.send(env);
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let (s, r2) = store.snapshot_and_subscribe();
                            mirror = s;
                            rx = r2;
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
                let _ = state_tx.send(Arc::new(mirror.clone()));
            }
        });
        Self {
            engine,
            state,
            events,
            link,
            cancel,
        }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[async_trait::async_trait]
impl Backend for Local {
    fn state(&self) -> watch::Receiver<Arc<State>> {
        self.state.clone()
    }
    fn events(&self) -> broadcast::Receiver<Arc<Envelope>> {
        self.events.subscribe()
    }
    fn link(&self) -> watch::Receiver<Link> {
        self.link.clone()
    }
    async fn execute(&self, cmd: Command) -> ApiResult {
        self.engine.execute(cmd).await
    }
}

// ------------------------------------------------------------------ remote

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<ApiResult>>>>;

/// A daemon reached through `tm-server`'s WebSocket. Reconnects forever.
pub struct Remote {
    state: watch::Receiver<Arc<State>>,
    events: broadcast::Sender<Arc<Envelope>>,
    link: watch::Receiver<Link>,
    out: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: AtomicU64,
    cancel: CancellationToken,
}

impl Remote {
    /// `base_url` like `http://192.168.1.10:7878`. Returns once the first snapshot arrived
    /// (or after 10 s, in which case the mirror fills in when the daemon becomes reachable).
    pub async fn connect(base_url: &str, token: &str) -> Result<Self, ApiError> {
        tm_twitch::tls::install_crypto_provider();
        let ws_url = format!(
            "{}/api/v1/ws?token={token}",
            base_url.trim_end_matches('/').replacen("http", "ws", 1)
        );
        let (state_tx, state) = watch::channel(Arc::new(State::default()));
        let (events, _) = broadcast::channel(1024);
        let (link_tx, link) = watch::channel(Link::Connecting);
        let (out, out_rx) = mpsc::unbounded_channel();
        let pending: Pending = Arc::default();
        let cancel = CancellationToken::new();
        tokio::spawn(run_remote(
            ws_url,
            state_tx,
            events.clone(),
            link_tx,
            out_rx,
            pending.clone(),
            cancel.clone(),
        ));
        // Wait for the first snapshot, or the first failure, at most 10 s.
        let mut first = state.clone();
        let mut l = link.clone();
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::select! {
                _ = first.changed() => {}
                _ = async {
                    while l.changed().await.is_ok() {
                        if matches!(*l.borrow(), Link::Disconnected(_)) { break; }
                    }
                } => {}
            }
        })
        .await;
        Ok(Self {
            state,
            events,
            link,
            out,
            pending,
            next_id: AtomicU64::new(1),
            cancel,
        })
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[async_trait::async_trait]
impl Backend for Remote {
    fn state(&self) -> watch::Receiver<Arc<State>> {
        self.state.clone()
    }
    fn events(&self) -> broadcast::Receiver<Arc<Envelope>> {
        self.events.subscribe()
    }
    fn link(&self) -> watch::Receiver<Link> {
        self.link.clone()
    }
    async fn execute(&self, cmd: Command) -> ApiResult {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let msg = json!({ "id": id, "cmd": cmd }).to_string();
        if self.out.send(msg).is_err() {
            self.pending.lock().remove(&id);
            return Err(ApiError::new("unavailable", "client closed"));
        }
        match tokio::time::timeout(Duration::from_secs(150), rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(ApiError::new("unavailable", "connection lost")),
            Err(_) => {
                self.pending.lock().remove(&id);
                Err(ApiError::new("timeout", "no reply from daemon"))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_remote(
    url: String,
    state_tx: watch::Sender<Arc<State>>,
    events: broadcast::Sender<Arc<Envelope>>,
    link: watch::Sender<Link>,
    mut out_rx: mpsc::UnboundedReceiver<String>,
    pending: Pending,
    cancel: CancellationToken,
) {
    let net = match tm_twitch::Net::new(&tm_twitch::NetConfig::default()) {
        Ok(n) => tm_twitch::Net {
            ws_proxy: None,
            ..n
        },
        Err(e) => {
            let _ = link.send(Link::Disconnected(e.to_string()));
            return;
        }
    };
    let mut backoff =
        tm_twitch::net::Backoff::new(Duration::from_millis(500), Duration::from_secs(30));
    loop {
        let _ = link.send(Link::Connecting);
        let ws = tokio::select! {
            _ = cancel.cancelled() => return,
            r = tm_twitch::ws::connect(&net, &url) => r,
        };
        let ws = match ws {
            Ok(ws) => ws,
            Err(e) => {
                let _ = link.send(Link::Disconnected(e.to_string()));
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(backoff.next_delay()) => continue,
                }
            }
        };
        backoff.reset();
        let (mut sink, mut stream) = ws.split();
        let mut mirror: Option<State> = None;
        let mut resyncing = false;
        let reason = loop {
            tokio::select! {
                _ = cancel.cancelled() => { let _ = sink.close().await; return; }
                Some(msg) = out_rx.recv() => {
                    if sink.send(Message::text(msg)).await.is_err() { break "send failed".to_string(); }
                }
                msg = stream.next() => {
                    let text = match msg {
                        Some(Ok(Message::Text(t))) => t,
                        Some(Ok(Message::Close(_))) | None => break "closed".to_string(),
                        Some(Err(e)) => break e.to_string(),
                        Some(Ok(_)) => continue,
                    };
                    let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                    match v["type"].as_str() {
                        Some("snapshot") => {
                            if let Ok(s) = serde_json::from_value::<State>(v["state"].clone()) {
                                let _ = state_tx.send(Arc::new(s.clone()));
                                mirror = Some(s);
                                let _ = link.send(Link::Connected);
                            }
                        }
                        Some("event") => {
                            let Some(m) = mirror.as_mut() else { continue };
                            match serde_json::from_value::<Envelope>(v["event"].clone()) {
                                // In order: apply.
                                Ok(env) if env.seq == m.seq + 1 => {
                                    apply_envelope(m, &env);
                                    let _ = events.send(Arc::new(env));
                                    let _ = state_tx.send(Arc::new(m.clone()));
                                }
                                Ok(env) if env.seq <= m.seq => {}
                                // A gap, or an event type this client does not know (newer
                                // daemon): resynchronise from a fresh snapshot.
                                _ => {
                                    if !resyncing {
                                        resyncing = true;
                                        let req = json!({"id": RESYNC_ID, "cmd": {"cmd": "snapshot"}}).to_string();
                                        if sink.send(Message::text(req)).await.is_err() { break "send failed".to_string(); }
                                    }
                                }
                            }
                        }
                        Some("reply") if v["id"].as_u64() == Some(RESYNC_ID) => {
                            if let Ok(s) = serde_json::from_value::<State>(v["data"].clone()) {
                                let _ = state_tx.send(Arc::new(s.clone()));
                                mirror = Some(s);
                            }
                            resyncing = false;
                        }
                        Some("reply") => {
                            let Some(id) = v["id"].as_u64() else { continue };
                            let Some(tx) = pending.lock().remove(&id) else { continue };
                            let r = if v["ok"].as_bool() == Some(true) {
                                Ok(v["data"].clone())
                            } else {
                                Err(serde_json::from_value(v["error"].clone())
                                    .unwrap_or_else(|_| ApiError::new("unknown", v["error"].to_string())))
                            };
                            let _ = tx.send(r);
                        }
                        _ => {}
                    }
                }
            }
        };
        let _ = link.send(Link::Disconnected(reason));
        // Fail in-flight commands: their replies are lost with the connection.
        pending.lock().clear();
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(backoff.next_delay()) => {}
        }
    }
}

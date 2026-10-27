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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tm_core::logging::{LogBuffer, LogLine};
use tm_core::state::apply_envelope;
use tm_core::{ApiError, ApiResult, Command, Engine, Envelope, State};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

mod fault;
pub use fault::{Fault, clean_token, diagnose, normalize_url};

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
    /// The engine's last `n` log lines, oldest first (poll it while a log view is open).
    async fn logs(&self, _n: usize) -> Result<Vec<LogLine>, ApiError> {
        Ok(Vec::new())
    }
    /// Connected with a guest token: only the holder's accounts are visible, and only
    /// their settings (`account:<id>/module:*`) can be changed.
    fn guest(&self) -> bool {
        false
    }
}

/// Request id reserved for internal snapshot resyncs (user ids start at 1).
const RESYNC_ID: u64 = 0;

/// Publishes the mirror at most this often (coalesces bursts of events).
const PUBLISH_EVERY: Duration = Duration::from_millis(50);
/// The remote client pings the engine this often…
const PING_EVERY: Duration = Duration::from_secs(20);
/// …and gives the connection up after this long without hearing from it.
const SILENCE_LIMIT: Duration = Duration::from_secs(60);

// ------------------------------------------------------------------- local

pub struct Local {
    engine: Engine,
    logs: Option<LogBuffer>,
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
            logs: None,
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
    async fn logs(&self, n: usize) -> Result<Vec<LogLine>, ApiError> {
        Ok(self.logs.as_ref().map(|l| l.recent(n)).unwrap_or_default())
    }
}

impl Local {
    /// Serves [`Backend::logs`] from the application's log buffer (`tm_app::App::logs`).
    pub fn with_logs(mut self, logs: LogBuffer) -> Self {
        self.logs = Some(logs);
        self
    }
}

// ------------------------------------------------------------------ remote

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<ApiResult>>>>;

/// A daemon reached through `tm-server`'s WebSocket. Reconnects forever.
pub struct Remote {
    base_url: String,
    token: String,
    http: reqwest::Client,
    state: watch::Receiver<Arc<State>>,
    events: broadcast::Sender<Arc<Envelope>>,
    link: watch::Receiver<Link>,
    out: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: AtomicU64,
    cancel: CancellationToken,
    guest: Arc<AtomicBool>,
}

impl Remote {
    /// `base_url` like `http://192.168.1.10:7878` (`http://` may be left out). Returns once
    /// the first snapshot arrived (or after 10 s, in which case the mirror fills in when the
    /// daemon becomes reachable). [`diagnose`] explains a `Link::Disconnected` reason.
    /// Spaces and line breaks pasted with the token are ignored.
    pub async fn connect(base_url: &str, token: &str) -> Result<Self, ApiError> {
        tm_twitch::tls::install_crypto_provider();
        let base_url = normalize_url(base_url);
        let token = clean_token(token);
        let ws_url = format!(
            "{}/api/v1/ws?token={}",
            base_url.replacen("http", "ws", 1),
            fault::query_value(&token)
        );
        let (state_tx, state) = watch::channel(Arc::new(State::default()));
        let (events, _) = broadcast::channel(1024);
        let (link_tx, link) = watch::channel(Link::Connecting);
        let (out, out_rx) = mpsc::unbounded_channel();
        let pending: Pending = Arc::default();
        let cancel = CancellationToken::new();
        let guest = Arc::new(AtomicBool::new(false));
        tokio::spawn(run_remote(
            ws_url,
            state_tx,
            events.clone(),
            link_tx,
            out_rx,
            pending.clone(),
            cancel.clone(),
            guest.clone(),
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
        // The engine's TLS settings: reqwest's default verifier panics on Android.
        let http = reqwest::Client::builder()
            .use_preconfigured_tls((*tm_twitch::tls::client_config()).clone())
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| ApiError::new("internal", e.to_string()))?;
        Ok(Self {
            base_url,
            token,
            http,
            state,
            events,
            link,
            out,
            pending,
            next_id: AtomicU64::new(1),
            cancel,
            guest,
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
    fn guest(&self) -> bool {
        self.guest.load(Ordering::Relaxed)
    }
    fn events(&self) -> broadcast::Receiver<Arc<Envelope>> {
        self.events.subscribe()
    }
    fn link(&self) -> watch::Receiver<Link> {
        self.link.clone()
    }
    async fn execute(&self, cmd: Command) -> ApiResult {
        // Refused at once rather than run whenever the link comes back.
        if !matches!(*self.link.borrow(), Link::Connected) {
            return Err(ApiError::new("unavailable", "not connected to the engine"));
        }
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
    async fn logs(&self, n: usize) -> Result<Vec<LogLine>, ApiError> {
        let unavailable = |e: reqwest::Error| ApiError::new("unavailable", e.to_string());
        let resp = self
            .http
            .get(format!("{}/api/v1/logs", self.base_url))
            .query(&[("n", n)])
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(unavailable)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body: Value = resp.json().await.unwrap_or_default();
            let code = body["error"]["code"].as_str().unwrap_or("unavailable");
            let msg = body["error"]["message"]
                .as_str()
                .unwrap_or("request refused");
            return Err(ApiError::new(code, format!("{status}: {msg}")));
        }
        resp.json().await.map_err(unavailable)
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
    guest: Arc<AtomicBool>,
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
        // Events received while a resync snapshot is on its way, applied after it.
        let mut resync: Option<Vec<Value>> = None;
        // Coalesces bursts of events into one state publication.
        let mut publish_at: Option<tokio::time::Instant> = None;
        let mut ping = tokio::time::interval(PING_EVERY);
        let mut last_seen = tokio::time::Instant::now();
        let reason = loop {
            tokio::select! {
                _ = cancel.cancelled() => { let _ = sink.close().await; return; }
                Some(msg) = out_rx.recv() => {
                    if sink.send(Message::text(msg)).await.is_err() { break "send failed".to_string(); }
                }
                _ = ping.tick() => {
                    // A half-open connection (sleep, NAT timeout) never errors: the
                    // missing answers to our pings tell.
                    if last_seen.elapsed() > SILENCE_LIMIT {
                        break "no answer from the engine".to_string();
                    }
                    if sink.send(Message::Ping(Default::default())).await.is_err() { break "send failed".to_string(); }
                }
                _ = sleep_until_opt(publish_at) => {
                    publish_at = None;
                    if let Some(m) = &mirror {
                        let _ = state_tx.send(Arc::new(m.clone()));
                    }
                }
                msg = stream.next() => {
                    last_seen = tokio::time::Instant::now();
                    let text = match msg {
                        Some(Ok(Message::Text(t))) => t,
                        Some(Ok(Message::Close(_))) | None => break "closed".to_string(),
                        Some(Err(e)) => break e.to_string(),
                        Some(Ok(_)) => continue,
                    };
                    let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                    match v["type"].as_str() {
                        Some("snapshot") => {
                            guest.store(v["access"].as_str() == Some("guest"), Ordering::Relaxed);
                            if let Ok(s) = serde_json::from_value::<State>(v["state"].clone()) {
                                // Link first: whoever sees the new state also sees `Connected`.
                                let _ = link.send(Link::Connected);
                                let _ = state_tx.send(Arc::new(s.clone()));
                                mirror = Some(s);
                            }
                        }
                        Some("event") => {
                            if let Some(buffer) = resync.as_mut() {
                                buffer.push(v["event"].clone());
                                continue;
                            }
                            let Some(m) = mirror.as_mut() else { continue };
                            match apply_remote(m, &v["event"], &events, guest.load(Ordering::Relaxed)) {
                                Applied::Yes => {
                                    publish_at.get_or_insert_with(|| tokio::time::Instant::now() + PUBLISH_EVERY);
                                }
                                Applied::Old => {}
                                // A gap, or an event type this client does not know (newer
                                // daemon): resynchronise from a fresh snapshot.
                                Applied::Gap => {
                                    resync = Some(Vec::new());
                                    let req = json!({"id": RESYNC_ID, "cmd": {"cmd": "snapshot"}}).to_string();
                                    if sink.send(Message::text(req)).await.is_err() { break "send failed".to_string(); }
                                }
                            }
                        }
                        Some("reply") if v["id"].as_u64() == Some(RESYNC_ID) => {
                            let buffered = resync.take().unwrap_or_default();
                            let Ok(mut s) = serde_json::from_value::<State>(v["data"].clone()) else { continue };
                            // The snapshot already holds some of the buffered events.
                            let mut gap = false;
                            for e in &buffered {
                                if apply_remote(&mut s, e, &events, guest.load(Ordering::Relaxed)) == Applied::Gap {
                                    gap = true;
                                    break;
                                }
                            }
                            let _ = state_tx.send(Arc::new(s.clone()));
                            mirror = Some(s);
                            publish_at = None;
                            if gap {
                                resync = Some(Vec::new());
                                let req = json!({"id": RESYNC_ID, "cmd": {"cmd": "snapshot"}}).to_string();
                                if sink.send(Message::text(req)).await.is_err() { break "send failed".to_string(); }
                            }
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
        // Fail in-flight commands (their replies are lost with the connection), and
        // drop unsent ones: their callers were told the link is down.
        pending.lock().clear();
        while out_rx.try_recv().is_ok() {}
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(backoff.next_delay()) => {}
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Applied {
    Yes,
    /// Already in the mirror.
    Old,
    /// Out of order or unknown to this client: needs a fresh snapshot.
    Gap,
}

/// Applies one event received from the engine to `mirror` and forwards it. `gaps`:
/// a guest's stream skips the events of other accounts, so a jump in `seq` is normal.
fn apply_remote(
    mirror: &mut State,
    event: &Value,
    events: &broadcast::Sender<Arc<Envelope>>,
    gaps: bool,
) -> Applied {
    if event["seq"].as_u64().is_some_and(|seq| seq <= mirror.seq) {
        return Applied::Old;
    }
    match serde_json::from_value::<Envelope>(event.clone()) {
        Ok(env) if env.seq == mirror.seq + 1 || (gaps && env.seq > mirror.seq) => {
            apply_envelope(mirror, &env);
            let _ = events.send(Arc::new(env));
            Applied::Yes
        }
        _ => Applied::Gap,
    }
}

async fn sleep_until_opt(t: Option<tokio::time::Instant>) {
    match t {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending().await,
    }
}

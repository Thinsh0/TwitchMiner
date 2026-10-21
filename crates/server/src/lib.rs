//! Local API over HTTP + WebSocket. Same commands and events as the in-process API.
//!
//! | Route                  | Description                                            |
//! |------------------------|--------------------------------------------------------|
//! | `GET  /`               | built-in status page (asks for the token)              |
//! | `GET  /api/v1/health`  | liveness, no auth                                      |
//! | `GET  /api/v1/state`   | full state snapshot                                    |
//! | `POST /api/v1/command` | body = `Command` JSON, returns `{ok, data|error}`       |
//! | `GET  /api/v1/logs`    | recent log lines (`?n=200`)                            |
//! | `GET  /api/v1/ws`      | snapshot + live events + commands (`?logs=1` for logs) |
//! | `POST /api/v1/mcp`     | Model Context Protocol endpoint for AI agents          |
//!
//! Auth: `Authorization: Bearer <token>` or `?token=<token>` (for browsers' WebSocket).
//! The full token can do everything; the agent token is restricted (see `access`).

pub mod access;
mod mcp;

pub use access::Access;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tm_core::logging::LogBuffer;
use tm_core::{ApiResult, Command, Engine};
use tokio::sync::broadcast::error::RecvError;

#[derive(Clone)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    /// `None` disables authentication (only sensible on a loopback address).
    pub token: Option<String>,
    /// Restricted token for AI agents (see [`access`]).
    pub agent_token: Option<String>,
    pub logs: Option<LogBuffer>,
}

#[derive(Clone)]
struct AppState {
    engine: Engine,
    token: Option<Arc<str>>,
    agent_token: Option<Arc<str>>,
    logs: Option<LogBuffer>,
}

/// Bumped on breaking changes of the HTTP/WS protocol (additive changes keep it).
pub const API_VERSION: u32 = 1;

/// Generates a random API token.
pub fn generate_token() -> String {
    (0..4)
        .map(|_| format!("{:016x}", rand::random::<u64>()))
        .collect()
}

pub fn router(engine: Engine, cfg: &ServerConfig) -> Router {
    let state = AppState {
        engine,
        token: cfg.token.as_deref().map(Arc::from),
        agent_token: cfg.agent_token.as_deref().map(Arc::from),
        logs: cfg.logs.clone(),
    };
    let api = Router::new()
        .route("/state", get(get_state))
        .route("/command", post(post_command))
        .route("/logs", get(get_logs))
        .route("/ws", get(ws_upgrade))
        .route("/mcp", post(mcp_post).get(mcp_get))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        .route("/", get(dashboard))
        .route("/api/v1/health", get(health))
        .nest("/api/v1", api)
        .layer(middleware::from_fn(cors))
        .with_state(state)
}

/// Serves until `shutdown` resolves.
pub async fn serve(
    engine: Engine,
    cfg: ServerConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    serve_on(listener, engine, cfg, shutdown).await
}

/// Like [`serve`] with an already bound listener (lets callers report bind errors early).
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    engine: Engine,
    cfg: ServerConfig,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let app = router(engine, &cfg);
    tracing::info!(addr = %listener.local_addr()?, auth = cfg.token.is_some(), "API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

async fn auth(
    State(st): State<AppState>,
    Query(q): Query<TokenQuery>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(full) = &st.token else {
        req.extensions_mut().insert(Access::Full);
        return next.run(req).await;
    };
    let header_token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let given = header_token.or(q.token).unwrap_or_default();
    let matches = |t: &str| constant_time_eq(given.as_bytes(), t.as_bytes());
    let access = if matches(full) {
        Some(Access::Full)
    } else if st.agent_token.as_deref().is_some_and(matches) {
        Some(Access::Agent)
    } else {
        None
    };
    match access {
        Some(Access::Agent) if !agent_enabled(&st.engine) => (
            StatusCode::FORBIDDEN,
            Json(json!({"ok": false, "error": {"code": "forbidden", "message": AGENT_DISABLED}})),
        )
            .into_response(),
        Some(a) => {
            req.extensions_mut().insert(a);
            next.run(req).await
        }
        None => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"ok": false, "error": {"code": "unauthorized", "message": "missing or bad token"}})),
        )
            .into_response(),
    }
}

/// WebSocket clients are pinged this often, and dropped after this long in silence.
const WS_PING_EVERY: std::time::Duration = std::time::Duration::from_secs(30);
const WS_SILENCE_LIMIT: std::time::Duration = std::time::Duration::from_secs(90);

const AGENT_DISABLED: &str =
    "AI agent access is disabled (set engine.agent_enabled to true with the full token)";

/// `engine.agent_enabled`, read live so toggling it needs no restart.
fn agent_enabled(engine: &Engine) -> bool {
    engine.store().read(|s| {
        s.settings
            .get(tm_core::settings::ENGINE_SCOPE)
            .and_then(|e| e.get("agent_enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    })
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn cors(req: Request, next: Next) -> Response {
    if req.method() == Method::OPTIONS {
        let mut r = StatusCode::NO_CONTENT.into_response();
        add_cors(&mut r);
        return r;
    }
    let mut r = next.run(req).await;
    add_cors(&mut r);
    r
}

fn add_cors(r: &mut Response) {
    let h = r.headers_mut();
    h.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
}

/// Minimal built-in status page (uses only the public WebSocket API).
async fn dashboard() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("dashboard.html"),
    )
}

async fn health(State(st): State<AppState>) -> Json<Value> {
    let (version, seq, instance_id) = st
        .engine
        .store()
        .read(|s| (s.version.clone(), s.seq, s.instance_id.clone()));
    Json(json!({
        "ok": true, "version": version, "api_version": API_VERSION, "seq": seq, "instance_id": instance_id,
    }))
}

async fn get_state(
    State(st): State<AppState>,
    Extension(access): Extension<Access>,
) -> Json<Value> {
    let mut state = json!(st.engine.store().snapshot());
    if access == Access::Agent {
        access::redact_state(&mut state);
    }
    Json(state)
}

/// Executes a command with access control; agents get secrets masked in the answer.
pub async fn run_command(engine: &Engine, access: Access, cmd: Command) -> ApiResult {
    access::authorize(access, &cmd)?;
    let (get_scope, set_scope) = match &cmd {
        Command::SettingsGet { scope, .. } => (Some(scope.clone()), None),
        Command::SettingsSet { scope, .. } => (None, Some(scope.clone())),
        _ => (None, None),
    };
    let is_snapshot = matches!(cmd, Command::Snapshot);
    let mut out = engine.execute(cmd).await?;
    if access == Access::Agent {
        if is_snapshot {
            access::redact_state(&mut out);
        }
        if let Some(scope) = get_scope {
            for k in ["raw", "effective"] {
                access::redact_settings(&scope, &mut out[k]);
            }
        }
        // `settings_set` answers with the whole merged document.
        if let Some(scope) = set_scope {
            access::redact_settings(&scope, &mut out);
        }
    }
    Ok(out)
}

fn reply(r: tm_core::ApiResult) -> Value {
    match r {
        Ok(data) => json!({ "ok": true, "data": data }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

async fn post_command(
    State(st): State<AppState>,
    Extension(access): Extension<Access>,
    body: axum::body::Bytes,
) -> Response {
    match serde_json::from_slice::<Command>(&body) {
        Ok(cmd) => {
            let r = run_command(&st.engine, access, cmd).await;
            let status = if r.is_ok() {
                StatusCode::OK
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, Json(reply(r))).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": {"code": "bad_command", "message": e.to_string()}})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct LogsQuery {
    n: Option<usize>,
}

async fn get_logs(State(st): State<AppState>, Query(q): Query<LogsQuery>) -> Json<Value> {
    let lines = st
        .logs
        .as_ref()
        .map(|l| l.recent(q.n.unwrap_or(200).min(1000)))
        .unwrap_or_default();
    Json(json!(lines))
}

#[derive(Deserialize)]
struct WsQuery {
    #[serde(default)]
    logs: Option<u8>,
}

async fn ws_upgrade(
    State(st): State<AppState>,
    Extension(access): Extension<Access>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let logs = q.logs.unwrap_or(0) == 1;
    ws.on_upgrade(move |socket| ws_session(st, access, socket, logs))
}

/// MCP Streamable HTTP: JSON-RPC message or batch in, JSON out (202 for notifications only).
async fn mcp_post(
    State(st): State<AppState>,
    Extension(access): Extension<Access>,
    body: axum::body::Bytes,
) -> Response {
    if !agent_enabled(&st.engine) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"ok": false, "error": {"code": "not_found", "message": AGENT_DISABLED}})),
        )
            .into_response();
    }
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            let err = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}});
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
    };
    let out = match msg {
        Value::Array(batch) => {
            let mut replies = Vec::new();
            for m in batch {
                if let Some(r) = mcp::handle(&st.engine, access, m).await {
                    replies.push(r);
                }
            }
            (!replies.is_empty()).then_some(Value::Array(replies))
        }
        m => mcp::handle(&st.engine, access, m).await,
    };
    match out {
        Some(v) => Json(v).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// No server-initiated stream: clients fall back to plain request/response.
async fn mcp_get() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

fn snapshot_msg(access: Access, state: &tm_core::State) -> String {
    let mut s = json!(state);
    if access == Access::Agent {
        access::redact_state(&mut s);
    }
    json!({"type": "snapshot", "api_version": API_VERSION, "state": s}).to_string()
}

/// Client → server: `{ "id": <any>, "cmd": { "cmd": "...", ... } }`.
#[derive(Deserialize)]
struct WsRequest {
    #[serde(default)]
    id: Value,
    cmd: Value,
}

async fn ws_session(st: AppState, access: Access, socket: WebSocket, with_logs: bool) {
    let (mut tx, mut rx) = socket.split();
    let (snapshot, mut events) = st.engine.store().snapshot_and_subscribe();
    let mut last_seq = snapshot.seq;
    if tx
        .send(Message::text(snapshot_msg(access, &snapshot)))
        .await
        .is_err()
    {
        return;
    }
    let mut logs = st
        .logs
        .as_ref()
        .filter(|_| with_logs)
        .map(|l| l.subscribe());
    let (reply_tx, mut reply_rx) = tokio::sync::mpsc::channel::<Value>(32);
    // Pings find clients that vanished without closing (sleep, network change).
    let mut ping = tokio::time::interval(WS_PING_EVERY);
    let mut last_seen = tokio::time::Instant::now();

    loop {
        tokio::select! {
            _ = ping.tick() => {
                if last_seen.elapsed() > WS_SILENCE_LIMIT { return; }
                if tx.send(Message::Ping(Default::default())).await.is_err() { return; }
            }
            ev = events.recv() => match ev {
                Ok(env) => {
                    if env.seq <= last_seq { continue; }
                    last_seq = env.seq;
                    if access == Access::Agent && !agent_enabled(&st.engine) { return; }
                    // `event` is an `Envelope`: {seq, ts, type, ...fields}.
                    let mut event = json!(env.as_ref());
                    if access == Access::Agent {
                        access::redact_envelope(&env.event, &mut event);
                    }
                    let msg = json!({"type": "event", "event": event});
                    if tx.send(Message::text(msg.to_string())).await.is_err() { return; }
                }
                Err(RecvError::Lagged(_)) => {
                    // Too slow: resynchronise with a fresh snapshot.
                    let (s, rx2) = st.engine.store().snapshot_and_subscribe();
                    events = rx2;
                    last_seq = s.seq;
                    if tx.send(Message::text(snapshot_msg(access, &s))).await.is_err() { return; }
                }
                Err(RecvError::Closed) => return,
            },
            line = async { match logs.as_mut() { Some(l) => l.recv().await.ok(), None => std::future::pending().await } } => {
                if let Some(line) = line
                    && tx.send(Message::text(json!({"type": "log", "line": line}).to_string())).await.is_err() { return; }
            }
            Some(r) = reply_rx.recv() => {
                if tx.send(Message::text(r.to_string())).await.is_err() { return; }
            }
            msg = rx.next() => {
                last_seen = tokio::time::Instant::now();
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                    Some(Ok(_)) => continue,
                };
                let req: WsRequest = match serde_json::from_str(&text) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = reply_tx.send(json!({"type": "reply", "id": null, "ok": false, "error": {"code": "bad_request", "message": e.to_string()}})).await;
                        continue;
                    }
                };
                // Commands run concurrently so a slow one doesn't block the event stream.
                let engine = st.engine.clone();
                let out = reply_tx.clone();
                tokio::spawn(async move {
                    let mut r = match serde_json::from_value::<Command>(req.cmd) {
                        Ok(cmd) => reply(run_command(&engine, access, cmd).await),
                        Err(e) => json!({"ok": false, "error": {"code": "bad_command", "message": e.to_string()}}),
                    };
                    r["type"] = json!("reply");
                    r["id"] = req.id;
                    let _ = out.send(r).await;
                });
            }
        }
    }
}

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
//! Auth: `Authorization: Bearer <token>`. The WebSocket also accepts the token as its
//! first message (`{"type": "auth", "token": …}`, for browsers, which cannot set
//! headers). `?token=<token>` in the URL still works but is deprecated (URLs end up in
//! logs and histories).
//! The full token can do everything; the agent token is restricted (see `access`); a
//! guest token only sees and changes its holder's accounts (see `guest`). Every route
//! is behind `guard` (Host, Origin, CORS, lockout after repeated bad tokens).

pub mod access;
mod guard;
pub mod guest;
mod mcp;

pub use guard::{FIRST_LOCK, FREE_FAILURES, MAX_LOCK};

pub use access::Access;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
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
    /// `None` disables authentication: refused by [`serve`] unless the address is a
    /// loopback one.
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
    strikes: Arc<guard::Strikes>,
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
        strikes: Arc::default(),
    };
    let api = Router::new()
        .route("/state", get(get_state))
        .route("/command", post(post_command))
        .route("/logs", get(get_logs))
        .route("/mcp", post(mcp_post).get(mcp_get))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        .route("/", get(dashboard))
        .route("/api/v1/health", get(health))
        // Authenticates itself: header, legacy `?token=`, or first message.
        .route("/api/v1/ws", get(ws_upgrade))
        .nest("/api/v1", api)
        .layer(middleware::from_fn_with_state(state.clone(), guard::guard))
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
    let addr = listener.local_addr()?;
    if cfg.token.is_none() && !addr.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "refusing to serve the API on {addr} without a token (only on a loopback address)"
            ),
        ));
    }
    let app = router(engine, &cfg);
    tracing::info!(%addr, auth = cfg.token.is_some(), "API listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

/// Why a token was not accepted.
enum AuthError {
    /// Too many bad tokens from this address lately.
    Locked(std::time::Duration),
    Bad,
    AgentDisabled,
}

impl AuthError {
    fn api_error(&self) -> tm_core::ApiError {
        match self {
            AuthError::Locked(wait) => tm_core::ApiError::new(
                "rate_limited",
                format!(
                    "too many bad tokens from this address: try again in {} s",
                    wait.as_secs().max(1)
                ),
            ),
            AuthError::Bad => tm_core::ApiError::new("unauthorized", "missing or bad token"),
            AuthError::AgentDisabled => tm_core::ApiError::new("forbidden", AGENT_DISABLED),
        }
    }

    fn response(&self) -> Response {
        let status = match self {
            AuthError::Locked(_) => StatusCode::TOO_MANY_REQUESTS,
            AuthError::Bad => StatusCode::UNAUTHORIZED,
            AuthError::AgentDisabled => StatusCode::FORBIDDEN,
        };
        let mut r = (
            status,
            Json(json!({"ok": false, "error": self.api_error()})),
        )
            .into_response();
        if let AuthError::Locked(wait) = self
            && let Ok(v) = wait.as_secs().max(1).to_string().parse()
        {
            r.headers_mut().insert(header::RETRY_AFTER, v);
        }
        r
    }
}

/// The token of an `Authorization: Bearer` header.
fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string())
}

/// Logs once that a client still sends its token in the URL (never the token itself).
fn note_token_in_url() {
    static NOTED: AtomicBool = AtomicBool::new(false);
    if !NOTED.swap(true, Ordering::Relaxed) {
        tracing::info!(
            "a client sends its API token in the URL (?token=, deprecated): update it to send the Authorization header"
        );
    }
}

/// Which access `given` grants, counting failures per client address.
fn authenticate(
    st: &AppState,
    ip: Option<IpAddr>,
    given: Option<&str>,
) -> Result<Access, AuthError> {
    if let Some(wait) = st.strikes.locked(ip) {
        return Err(AuthError::Locked(wait));
    }
    let Some(full) = &st.token else {
        return Ok(Access::Full);
    };
    let given = given.unwrap_or_default();
    let matches = |t: &str| constant_time_eq(given.as_bytes(), t.as_bytes());
    let access = if matches(full) {
        Some(Access::Full)
    } else if st.agent_token.as_deref().is_some_and(matches) {
        Some(Access::Agent)
    } else {
        st.engine
            .store()
            .read(|s| tm_core::guests::by_token(s, given))
            .map(|(id, _)| Access::Guest(id))
    };
    match access {
        Some(Access::Agent) if !agent_enabled(&st.engine) => Err(AuthError::AgentDisabled),
        Some(a) => {
            st.strikes.success(ip);
            Ok(a)
        }
        None => {
            st.strikes.failure(ip);
            Err(AuthError::Bad)
        }
    }
}

async fn auth(
    State(st): State<AppState>,
    Extension(guard::ClientIp(ip)): Extension<guard::ClientIp>,
    Query(q): Query<TokenQuery>,
    mut req: Request,
    next: Next,
) -> Response {
    let header_token = bearer(req.headers());
    if header_token.is_none() && q.token.is_some() {
        note_token_in_url();
    }
    let given = header_token.or(q.token);
    match authenticate(&st, ip, given.as_deref()) {
        Ok(a) => {
            req.extensions_mut().insert(a);
            next.run(req).await
        }
        Err(e) => e.response(),
    }
}

/// `true` when the body is declared as JSON: a web page cannot send that to another
/// site without the CORS check (a plain form can only send `text/plain` and the like).
fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
}

const NOT_JSON: &str = "Content-Type must be application/json";

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
    Json(state_for(&access, &st.engine.store().snapshot()))
}

/// The state as `access` may see it.
fn state_for(access: &Access, state: &tm_core::State) -> Value {
    match access {
        Access::Full => json!(state),
        Access::Agent => {
            let mut s = json!(state);
            access::redact_state(&mut s);
            s
        }
        Access::Guest(id) => match guest::View::of(state, id) {
            Some(view) => {
                let mut s = state.clone();
                view.filter_state(&mut s);
                json!(s)
            }
            None => json!(tm_core::State::default()),
        },
    }
}

fn guest_removed() -> tm_core::ApiError {
    tm_core::ApiError::new("unauthorized", "this guest access was revoked")
}

/// Executes a command with access control; agents get secrets masked in the answer,
/// guests only what is theirs.
pub async fn run_command(engine: &Engine, access: Access, cmd: Command) -> ApiResult {
    access::authorize(&access, &cmd)?;
    if let Access::Guest(id) = &access {
        let (view, cmd) = engine
            .store()
            .read(|s| {
                let view = guest::View::of(s, id)?;
                let cmd = view.authorize(cmd, s);
                Some((view, cmd))
            })
            .ok_or_else(guest_removed)?;
        let cmd = cmd?;
        let kind = guest::ReplyKind::of(&cmd);
        let mut out = engine.execute(cmd).await?;
        view.filter_reply(&kind, &mut out, &engine.store().snapshot());
        return Ok(out);
    }
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
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !is_json(&headers) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({"ok": false, "error": {"code": "bad_request", "message": NOT_JSON}})),
        )
            .into_response();
    }
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

async fn get_logs(
    State(st): State<AppState>,
    Extension(access): Extension<Access>,
    Query(q): Query<LogsQuery>,
) -> Response {
    // The log names every account: not for guests.
    if matches!(access, Access::Guest(_)) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"ok": false, "error": {"code": "forbidden", "message": "a guest token cannot read the log"}})),
        )
            .into_response();
    }
    let lines = st
        .logs
        .as_ref()
        .map(|l| l.recent(q.n.unwrap_or(200).min(1000)))
        .unwrap_or_default();
    Json(json!(lines)).into_response()
}

#[derive(Deserialize)]
struct WsQuery {
    #[serde(default)]
    logs: Option<u8>,
    /// Deprecated: prefer the `Authorization` header or the first message.
    #[serde(default)]
    token: Option<String>,
}

/// How long a WebSocket opened without a token may take to send it.
const WS_AUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

async fn ws_upgrade(
    State(st): State<AppState>,
    Extension(guard::ClientIp(ip)): Extension<guard::ClientIp>,
    headers: HeaderMap,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let header_token = bearer(&headers);
    let query_token = q.token.filter(|t| !t.is_empty());
    if header_token.is_none() && query_token.is_some() {
        note_token_in_url();
    }
    // A token with the request is checked now (a bad one answers 401, as before);
    // without one, the first message must carry it.
    let known = match header_token.or(query_token) {
        Some(t) => match authenticate(&st, ip, Some(&t)) {
            Ok(a) => Some(a),
            Err(e) => return e.response(),
        },
        None if st.token.is_none() => Some(Access::Full),
        None => {
            if let Some(wait) = st.strikes.locked(ip) {
                return AuthError::Locked(wait).response();
            }
            None
        }
    };
    let wants_logs = q.logs.unwrap_or(0) == 1;
    ws.on_upgrade(move |mut socket| async move {
        let access = match known {
            Some(a) => a,
            None => match ws_first_message_auth(&st, ip, &mut socket).await {
                Some(a) => a,
                None => return,
            },
        };
        let logs = wants_logs && !matches!(access, Access::Guest(_));
        ws_session(st, access, socket, logs).await
    })
}

/// Waits for `{"type": "auth", "token": …}`; on failure tells the client why and
/// closes the connection.
async fn ws_first_message_auth(
    st: &AppState,
    ip: Option<IpAddr>,
    socket: &mut WebSocket,
) -> Option<Access> {
    let token = loop {
        match tokio::time::timeout(WS_AUTH_TIMEOUT, socket.recv()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                break serde_json::from_str::<Value>(&t)
                    .ok()
                    .filter(|v| v["type"] == "auth")
                    .and_then(|v| v["token"].as_str().map(str::to_string));
            }
            // Pings and pongs before the token are fine.
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            Ok(Some(Ok(Message::Close(_))) | None | Some(Err(_))) => return None,
            _ => break None,
        }
    };
    match authenticate(st, ip, token.as_deref()) {
        Ok(a) => Some(a),
        Err(e) => {
            let msg = json!({"type": "error", "error": e.api_error()});
            let _ = socket.send(Message::text(msg.to_string())).await;
            let _ = socket
                .send(Message::Close(Some(CloseFrame {
                    code: 1008,
                    reason: "unauthorized".into(),
                })))
                .await;
            None
        }
    }
}

/// MCP Streamable HTTP: JSON-RPC message or batch in, JSON out (202 for notifications only).
async fn mcp_post(
    State(st): State<AppState>,
    Extension(access): Extension<Access>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !agent_enabled(&st.engine) || matches!(access, Access::Guest(_)) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"ok": false, "error": {"code": "not_found", "message": AGENT_DISABLED}})),
        )
            .into_response();
    }
    if !is_json(&headers) {
        let err =
            json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": NOT_JSON}});
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(err)).into_response();
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
                if let Some(r) = mcp::handle(&st.engine, access.clone(), m).await {
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

/// `access`: what the token may do; a guest's event stream has gaps in `seq` (the
/// events of other accounts are not sent).
fn snapshot_msg(access: &Access, state: &tm_core::State) -> String {
    json!({
        "type": "snapshot", "api_version": API_VERSION, "access": access.name(),
        "state": state_for(access, state),
    })
    .to_string()
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
        .send(Message::text(snapshot_msg(&access, &snapshot)))
        .await
        .is_err()
    {
        return;
    }
    // A guest's view changes as they add accounts, and ends when they are removed.
    let mut view = match &access {
        Access::Guest(id) => match guest::View::of(&snapshot, id) {
            Some(v) => Some(v),
            None => return,
        },
        _ => None,
    };
    let mut flows: std::collections::HashSet<String> = snapshot
        .logins
        .values()
        .filter(|f| {
            view.as_ref()
                .is_some_and(|v| f.guest.as_deref() == Some(v.id.as_str()))
        })
        .map(|f| f.flow_id.clone())
        .collect();
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
                    if let (Some(v), tm_core::Event::SettingsChanged { scope, .. }) = (&view, &env.event)
                        && scope == tm_core::guests::GUESTS_SCOPE
                    {
                        let (now, fresh) = st.engine.store().read(|s| (s.clone(), guest::View::of(s, &v.id)));
                        let Some(fresh) = fresh else { return };
                        if fresh != *v {
                            // An account was added or removed: start again from its state.
                            view = Some(fresh);
                            last_seq = now.seq;
                            if tx.send(Message::text(snapshot_msg(&access, &now))).await.is_err() { return; }
                        }
                        continue;
                    }
                    // `event` is an `Envelope`: {seq, ts, type, ...fields}.
                    let mut event = match &view {
                        Some(v) => match v.filter_event(&env, &mut flows) {
                            Some(e) => e,
                            None => continue,
                        },
                        None => json!(env.as_ref()),
                    };
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
                    if tx.send(Message::text(snapshot_msg(&access, &s))).await.is_err() { return; }
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
                    // A token sent again once authenticated (header): nothing to do.
                    Err(_) if serde_json::from_str::<Value>(&text).is_ok_and(|v| v["type"] == "auth") => continue,
                    Err(e) => {
                        let _ = reply_tx.send(json!({"type": "reply", "id": null, "ok": false, "error": {"code": "bad_request", "message": e.to_string()}})).await;
                        continue;
                    }
                };
                // Commands run concurrently so a slow one doesn't block the event stream.
                let engine = st.engine.clone();
                let out = reply_tx.clone();
                let access = access.clone();
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

//! Local API over HTTP + WebSocket. Same commands and events as the in-process API.
//!
//! | Route                  | Description                                            |
//! |------------------------|--------------------------------------------------------|
//! | `GET  /api/v1/health`  | liveness, no auth                                      |
//! | `GET  /api/v1/state`   | full state snapshot                                    |
//! | `POST /api/v1/command` | body = `Command` JSON, returns `{ok, data|error}`       |
//! | `GET  /api/v1/logs`    | recent log lines (`?n=200`)                            |
//! | `GET  /api/v1/ws`      | snapshot + live events + commands (`?logs=1` for logs) |
//!
//! Auth: `Authorization: Bearer <token>` or `?token=<token>` (for browsers' WebSocket).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tm_core::logging::LogBuffer;
use tm_core::{Command, Engine};
use tokio::sync::broadcast::error::RecvError;

#[derive(Clone)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    /// `None` disables authentication (only sensible on a loopback address).
    pub token: Option<String>,
    pub logs: Option<LogBuffer>,
}

#[derive(Clone)]
struct AppState {
    engine: Engine,
    token: Option<Arc<str>>,
    logs: Option<LogBuffer>,
}

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
        logs: cfg.logs.clone(),
    };
    let api = Router::new()
        .route("/state", get(get_state))
        .route("/command", post(post_command))
        .route("/logs", get(get_logs))
        .route("/ws", get(ws_upgrade))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
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
    req: Request,
    next: Next,
) -> Response {
    let Some(expected) = &st.token else {
        return next.run(req).await;
    };
    let header_token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let given = header_token.or(q.token);
    if given
        .as_deref()
        .is_some_and(|g| constant_time_eq(g.as_bytes(), expected.as_bytes()))
    {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({"ok": false, "error": {"code": "unauthorized", "message": "missing or bad token"}})))
            .into_response()
    }
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

async fn health(State(st): State<AppState>) -> Json<Value> {
    let (version, seq) = st.engine.store().read(|s| (s.version.clone(), s.seq));
    Json(json!({ "ok": true, "version": version, "seq": seq }))
}

async fn get_state(State(st): State<AppState>) -> Json<Value> {
    Json(json!(st.engine.store().snapshot()))
}

fn reply(r: tm_core::ApiResult) -> Value {
    match r {
        Ok(data) => json!({ "ok": true, "data": data }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

async fn post_command(State(st): State<AppState>, body: axum::body::Bytes) -> Response {
    match serde_json::from_slice::<Command>(&body) {
        Ok(cmd) => {
            let r = st.engine.execute(cmd).await;
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
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let logs = q.logs.unwrap_or(0) == 1;
    ws.on_upgrade(move |socket| ws_session(st, socket, logs))
}

/// Client → server: `{ "id": <any>, "cmd": { "cmd": "...", ... } }`.
#[derive(Deserialize)]
struct WsRequest {
    #[serde(default)]
    id: Value,
    cmd: Value,
}

async fn ws_session(st: AppState, socket: WebSocket, with_logs: bool) {
    let (mut tx, mut rx) = socket.split();
    let (snapshot, mut events) = st.engine.store().snapshot_and_subscribe();
    let mut last_seq = snapshot.seq;
    if tx
        .send(Message::text(
            json!({"type": "snapshot", "state": snapshot}).to_string(),
        ))
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

    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(env) => {
                    if env.seq <= last_seq { continue; }
                    last_seq = env.seq;
                    // `event` is an `Envelope`: {seq, ts, type, ...fields}.
                    let msg = json!({"type": "event", "event": env.as_ref()});
                    if tx.send(Message::text(msg.to_string())).await.is_err() { return; }
                }
                Err(RecvError::Lagged(_)) => {
                    // Too slow: resynchronise with a fresh snapshot.
                    let (s, rx2) = st.engine.store().snapshot_and_subscribe();
                    events = rx2;
                    last_seq = s.seq;
                    if tx.send(Message::text(json!({"type": "snapshot", "state": s}).to_string())).await.is_err() { return; }
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
                        Ok(cmd) => reply(engine.execute(cmd).await),
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

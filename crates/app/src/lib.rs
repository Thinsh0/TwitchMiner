//! One-call bootstrap shared by every front-end.
//!
//! ```ignore
//! let app = tm_app::App::start(tm_app::AppOptions::default()).await?;
//! let (state, events) = app.engine.store().snapshot_and_subscribe();
//! app.engine.execute(tm_core::Command::LoginStart { profile: None }).await?;
//! // ...
//! app.shutdown().await;
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use tm_core::logging::{self, LogBuffer, LogHandle, LogOptions};
use tm_core::{Engine, EngineConfig, Module, Paths};
use tokio::sync::Notify;

pub use tm_core;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct ApiOptions {
    pub bind: SocketAddr,
    /// `None`: use (or create) the token stored in the data directory.
    pub token: Option<String>,
}

impl Default for ApiOptions {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 7878)),
            token: None,
        }
    }
}

#[derive(Default)]
pub struct AppOptions {
    /// See [`Paths::resolve`]. On Android pass the app's files directory.
    pub data_dir: Option<PathBuf>,
    pub log_filter: Option<String>,
    pub log_stdout: bool,
    /// Start the HTTP/WebSocket API (needed for remote control, optional for a local UI).
    pub api: Option<ApiOptions>,
    /// Extra modules appended after the built-in ones.
    pub extra_modules: Vec<Arc<dyn Module>>,
    /// Application version reported in the state (defaults to this crate's version).
    pub version: Option<String>,
}

pub struct App {
    pub engine: Engine,
    pub paths: Paths,
    pub logs: LogBuffer,
    pub api_token: String,
    stop: Arc<Notify>,
    _log: LogHandle,
}

const TOKEN_KV: (&str, &str) = ("server", "token");

impl App {
    pub async fn start(opts: AppOptions) -> anyhow::Result<Self> {
        tm_twitch::tls::install_crypto_provider();
        let paths = Paths::resolve(opts.data_dir)?;
        let log = logging::init(
            &paths,
            LogOptions {
                filter: opts.log_filter,
                stdout: opts.log_stdout,
            },
        );
        let mut modules = tm_modules::builtin();
        modules.extend(opts.extra_modules);
        let engine = Engine::start(EngineConfig {
            paths: paths.clone(),
            modules,
            version: opts.version.unwrap_or_else(|| VERSION.to_string()),
        })
        .await?;

        let api_token = match opts.api.as_ref().and_then(|a| a.token.clone()) {
            Some(t) => t,
            None => stored_token(&engine).await?,
        };
        let stop = Arc::new(Notify::new());
        if let Some(api) = opts.api {
            let cfg = tm_server::ServerConfig {
                bind: api.bind,
                token: Some(api_token.clone()),
                logs: Some(log.buffer.clone()),
            };
            let engine = engine.clone();
            let stop = stop.clone();
            let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
            tokio::spawn(async move {
                if let Err(e) =
                    tm_server::serve_on(listener, engine, cfg, async move { stop.notified().await })
                        .await
                {
                    tracing::error!(error = %e, "API server stopped");
                }
            });
        }
        tracing::info!(version = VERSION, data = %paths.root.display(), "TwitchMiner started");
        Ok(Self {
            engine,
            paths,
            logs: log.buffer.clone(),
            api_token,
            stop,
            _log: log,
        })
    }

    /// Stops the API, every account and flushes the database.
    pub async fn shutdown(self) {
        self.stop.notify_waiters();
        self.engine.shutdown().await;
    }
}

async fn stored_token(engine: &Engine) -> anyhow::Result<String> {
    let db = engine.store().db();
    if let Some(Value::String(t)) = db.kv_get(TOKEN_KV.0.into(), TOKEN_KV.1.into()).await? {
        return Ok(t);
    }
    let t = tm_server::generate_token();
    db.kv_put(TOKEN_KV.0.into(), TOKEN_KV.1.into(), json!(t))
        .await?;
    Ok(t)
}

/// Reads (or creates) the API token without starting the engine.
pub async fn read_api_token(data_dir: Option<PathBuf>) -> anyhow::Result<String> {
    let paths = Paths::resolve(data_dir)?;
    let db = tm_core::db::Db::open(&paths.db_file())?;
    if let Some(Value::String(t)) = db.kv_get(TOKEN_KV.0.into(), TOKEN_KV.1.into()).await? {
        return Ok(t);
    }
    let t = tm_server::generate_token();
    db.kv_put(TOKEN_KV.0.into(), TOKEN_KV.1.into(), json!(t))
        .await?;
    Ok(t)
}

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
    /// Restricted token to give to AI agents (MCP endpoint).
    pub agent_token: String,
    stop: Arc<Notify>,
    _log: LogHandle,
}

const TOKEN_NS: &str = "server";

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
            None => token_in(engine.store().db(), TokenKind::Full).await?,
        };
        let agent_token = token_in(engine.store().db(), TokenKind::Agent).await?;
        let stop = Arc::new(Notify::new());
        if let Some(api) = opts.api {
            let cfg = tm_server::ServerConfig {
                bind: api.bind,
                token: Some(api_token.clone()),
                agent_token: Some(agent_token.clone()),
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
            agent_token,
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

/// Which API token to read or create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Full access (owner, apps).
    Full,
    /// Restricted access for AI agents (MCP).
    Agent,
}

impl TokenKind {
    fn key(self) -> &'static str {
        match self {
            TokenKind::Full => "token",
            TokenKind::Agent => "agent_token",
        }
    }
}

async fn token_in(db: &tm_core::db::Db, kind: TokenKind) -> anyhow::Result<String> {
    if let Some(Value::String(t)) = db.kv_get(TOKEN_NS.into(), kind.key().into()).await? {
        return Ok(t);
    }
    let t = tm_server::generate_token();
    db.kv_put(TOKEN_NS.into(), kind.key().into(), json!(t))
        .await?;
    Ok(t)
}

/// Reads (or creates) an API token without starting the engine.
pub async fn read_token(data_dir: Option<PathBuf>, kind: TokenKind) -> anyhow::Result<String> {
    let paths = Paths::resolve(data_dir)?;
    let db = tm_core::db::Db::open(&paths.db_file())?;
    token_in(&db, kind).await
}

/// Replaces an API token. A running daemon keeps the old one until it restarts.
pub async fn rotate_token(data_dir: Option<PathBuf>, kind: TokenKind) -> anyhow::Result<String> {
    let paths = Paths::resolve(data_dir)?;
    let db = tm_core::db::Db::open(&paths.db_file())?;
    let t = tm_server::generate_token();
    db.kv_put(TOKEN_NS.into(), kind.key().into(), json!(t))
        .await?;
    Ok(t)
}

const UI_NS: &str = "ui";
const UI_PREFS: &str = "prefs";

/// Preferences of a graphical interface (theme, language, remote server…): a free JSON
/// document owned by the front-end, kept in the database (not engine state, not exposed
/// by the API). Works without a local engine, and while one runs. `Null` when unset.
pub async fn read_ui_prefs(data_dir: Option<PathBuf>) -> anyhow::Result<Value> {
    let paths = Paths::resolve(data_dir)?;
    let db = tm_core::db::Db::open(&paths.db_file())?;
    Ok(db
        .kv_get(UI_NS.into(), UI_PREFS.into())
        .await?
        .unwrap_or(Value::Null))
}

/// Replaces the interface preferences (see [`read_ui_prefs`]).
pub async fn write_ui_prefs(data_dir: Option<PathBuf>, prefs: Value) -> anyhow::Result<()> {
    let paths = Paths::resolve(data_dir)?;
    let db = tm_core::db::Db::open(&paths.db_file())?;
    db.kv_put(UI_NS.into(), UI_PREFS.into(), prefs).await?;
    Ok(())
}

/// Reads (or creates) the full-access API token without starting the engine.
pub async fn read_api_token(data_dir: Option<PathBuf>) -> anyhow::Result<String> {
    read_token(data_dir, TokenKind::Full).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ui_prefs_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "tm-app-prefs-{}-{}",
            std::process::id(),
            chrono_free_nanos()
        ));
        assert_eq!(read_ui_prefs(Some(dir.clone())).await.unwrap(), Value::Null);
        let prefs = json!({"theme": "light", "remote": {"url": "http://h:7878"}});
        write_ui_prefs(Some(dir.clone()), prefs.clone())
            .await
            .unwrap();
        assert_eq!(read_ui_prefs(Some(dir.clone())).await.unwrap(), prefs);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn chrono_free_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    }
}

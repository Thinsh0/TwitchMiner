//! Where the engine runs: in this process (`Local`, owning a `tm_app::App`) or in a
//! daemon reached over the network (`Remote`). The rest of the interface only sees
//! the `Backend`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tm_client::{Backend, Link, Local, Remote};
use tm_core::engine::EngineError;
use tm_core::paths::LockError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Local,
    Remote {
        url: String,
        token: String,
    },
    /// Canned state for development and screenshots (`TM_UI_DEMO=1`).
    #[doc(hidden)]
    Demo,
}

pub struct Session {
    pub backend: Arc<dyn Backend>,
    pub target: Target,
    app: Option<tm_app::App>,
}

/// Logs and API tokens of a local engine.
#[derive(Clone)]
pub struct LocalInfo {
    pub logs: tm_core::logging::LogBuffer,
    pub agent_token: String,
    pub api_token: String,
}

#[derive(Debug)]
pub enum OpenError {
    /// Another engine (usually the daemon) holds the data directory.
    Busy {
        pid: String,
    },
    Failed(String),
}

/// Default address of a daemon running on this machine.
pub const LOCAL_DAEMON_URL: &str = "http://127.0.0.1:7878";

impl Session {
    /// `api`: for a local engine, also serve the HTTP/WebSocket API on this address
    /// (remote control from a phone, AI agents).
    pub async fn open(
        target: Target,
        data_dir: Option<PathBuf>,
        api: Option<std::net::SocketAddr>,
    ) -> Result<Self, OpenError> {
        match &target {
            Target::Local => {
                let app = tm_app::App::start(tm_app::AppOptions {
                    data_dir,
                    log_stdout: false,
                    api: api.map(|bind| tm_app::ApiOptions { bind, token: None }),
                    ..Default::default()
                })
                .await
                .map_err(|e| match e.downcast_ref::<EngineError>() {
                    Some(EngineError::Lock(LockError::Busy { pid })) => {
                        OpenError::Busy { pid: pid.clone() }
                    }
                    _ => OpenError::Failed(format!("{e:#}")),
                })?;
                Ok(Self {
                    backend: Arc::new(Local::new(app.engine.clone()).with_logs(app.logs.clone())),
                    target,
                    app: Some(app),
                })
            }
            Target::Demo => Ok(Self {
                backend: Arc::new(crate::demo::Demo::new()),
                target,
                app: None,
            }),
            Target::Remote { url, token } => {
                let remote = Remote::connect(url.trim(), token.trim())
                    .await
                    .map_err(|e| OpenError::Failed(e.message))?;
                // `connect` returns after the first snapshot or the first failure.
                let mut link = remote.link();
                let _ = tokio::time::timeout(Duration::from_secs(2), async {
                    while *link.borrow() == Link::Connecting {
                        if link.changed().await.is_err() {
                            break;
                        }
                    }
                })
                .await;
                let current = link.borrow().clone();
                match current {
                    Link::Connected => Ok(Self {
                        backend: Arc::new(remote),
                        target,
                        app: None,
                    }),
                    Link::Disconnected(reason) => Err(OpenError::Failed(reason)),
                    Link::Connecting => Err(OpenError::Failed("no answer".into())),
                }
            }
        }
    }

    /// What only a local engine has (`None` for a remote one).
    pub fn local(&self) -> Option<LocalInfo> {
        self.app.as_ref().map(|a| LocalInfo {
            logs: a.logs.clone(),
            agent_token: a.agent_token.clone(),
            api_token: a.api_token.clone(),
        })
    }

    /// Stops the local engine (flushes the database); drops the remote connection.
    pub async fn close(self) {
        drop(self.backend);
        if let Some(app) = self.app {
            app.shutdown().await;
        }
    }
}

/// The full-access token of the engine using `data_dir`, to pre-fill the remote form
/// when the daemon already runs on this computer.
pub async fn local_daemon_token(data_dir: Option<PathBuf>) -> Option<String> {
    tm_app::read_token(data_dir, tm_app::TokenKind::Full)
        .await
        .ok()
}

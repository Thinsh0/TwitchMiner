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
    Remote { url: String, token: String },
}

pub struct Session {
    pub backend: Arc<dyn Backend>,
    pub target: Target,
    app: Option<tm_app::App>,
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
    pub async fn open(target: Target, data_dir: Option<PathBuf>) -> Result<Self, OpenError> {
        match &target {
            Target::Local => {
                let app = tm_app::App::start(tm_app::AppOptions {
                    data_dir,
                    log_stdout: false,
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
                    backend: Arc::new(Local::new(app.engine.clone())),
                    target,
                    app: Some(app),
                })
            }
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

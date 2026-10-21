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
    /// Why the local engine runs without the API it was asked to serve.
    pub api_error: Option<String>,
}

/// API tokens of a local engine.
#[derive(Clone)]
pub struct LocalInfo {
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
    /// (remote control from a phone, AI agents). `keep_trying`: a remote engine that
    /// does not answer is kept (it reconnects by itself) instead of failing.
    pub async fn open(
        target: Target,
        data_dir: Option<PathBuf>,
        api: Option<std::net::SocketAddr>,
        keep_trying: bool,
    ) -> Result<Self, OpenError> {
        match &target {
            Target::Local => {
                let start = |api: Option<std::net::SocketAddr>| {
                    tm_app::App::start(tm_app::AppOptions {
                        data_dir: data_dir.clone(),
                        log_stdout: false,
                        api: api.map(|bind| tm_app::ApiOptions { bind, token: None }),
                        ..Default::default()
                    })
                };
                // A busy or not-yet-available API address must not keep the engine
                // from mining: start it without the API and say so.
                let (started, api_error) = match start(api).await {
                    Err(e) if e.downcast_ref::<tm_app::ApiBindError>().is_some() => {
                        tracing::warn!(error = %e, "starting the engine without its API");
                        (start(None).await, Some(e.to_string()))
                    }
                    r => (r, None),
                };
                let app = started.map_err(|e| match e.downcast_ref::<EngineError>() {
                    Some(EngineError::Lock(LockError::Busy { pid })) => {
                        OpenError::Busy { pid: pid.clone() }
                    }
                    _ => OpenError::Failed(format!("{e:#}")),
                })?;
                Ok(Self {
                    backend: Arc::new(Local::new(app.engine.clone()).with_logs(app.logs.clone())),
                    target,
                    app: Some(app),
                    api_error,
                })
            }
            Target::Demo => Ok(Self {
                backend: Arc::new(crate::demo::Demo::new()),
                target,
                app: None,
                api_error: None,
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
                    // A saved engine that is down keeps being retried: the view shows
                    // it unreachable instead of asking again which engine to use.
                    _ if keep_trying => Ok(Self {
                        backend: Arc::new(remote),
                        target,
                        app: None,
                        api_error: None,
                    }),
                    Link::Connected => Ok(Self {
                        backend: Arc::new(remote),
                        target,
                        app: None,
                        api_error: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_busy_api_port_does_not_stop_the_engine() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = taken.local_addr().unwrap();
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tm-ui-session-{n}"));
        let session = Session::open(Target::Local, Some(dir.clone()), Some(addr), false)
            .await
            .expect("the engine starts without its API");
        assert!(
            session
                .api_error
                .as_ref()
                .unwrap()
                .contains(&addr.to_string())
        );
        session.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }
}

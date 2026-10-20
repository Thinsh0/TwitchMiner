use std::time::Duration;

/// Errors produced by the Twitch layer.
///
/// Variants are coarse on purpose: callers mostly need to know whether to retry,
/// re-authenticate, or give up.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("http status {status} from {url}")]
    Http { status: u16, url: String },
    #[error("token is invalid or revoked")]
    Unauthorized,
    #[error("GQL operation `{0}` is unknown to Twitch (outdated hash?)")]
    PersistedQueryNotFound(String),
    #[error("GQL error in `{op}`: {message}")]
    Gql { op: String, message: String },
    #[error("unexpected response: {0}")]
    Decode(String),
    #[error("operation `{0}` missing from manifest")]
    UnknownOperation(String),
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("websocket error: {0}")]
    WebSocket(String),
    #[error("cancelled")]
    Cancelled,
}

impl Error {
    /// Whether retrying the same request later has a reasonable chance to succeed.
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Network(_) | Error::Timeout(_) | Error::WebSocket(_) => true,
            Error::Http { status, .. } => *status >= 500 || *status == 429,
            Error::Gql { message, .. } => is_transient_gql_message(message),
            _ => false,
        }
    }

    pub(crate) fn decode(msg: impl Into<String>) -> Self {
        Error::Decode(msg.into())
    }
}

pub(crate) fn is_transient_gql_message(message: &str) -> bool {
    matches!(
        message,
        "service error"
            | "service timeout"
            | "service unavailable"
            | "request cancelled"
            | "context deadline exceeded"
            | "server error"
            | "internal server error"
    )
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Error::Timeout(Duration::from_secs(0))
        } else if let Some(status) = e.status() {
            Error::Http {
                status: status.as_u16(),
                url: e.url().map(|u| u.to_string()).unwrap_or_default(),
            }
        } else if e.is_decode() {
            Error::Decode(e.to_string())
        } else {
            Error::Network(e.to_string())
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Decode(e.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

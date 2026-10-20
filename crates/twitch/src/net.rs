//! HTTP plumbing shared by every account: one connection pool, one TLS config,
//! one proxy setting.

use std::time::Duration;

use crate::error::Result;
use crate::tls;

#[derive(Debug, Clone, Default)]
pub struct NetConfig {
    /// Explicit proxy URL (`http://host:port`, `socks5://…` not supported for WS).
    /// When `None`, the standard `HTTPS_PROXY` / `ALL_PROXY` variables are honoured.
    pub proxy: Option<String>,
}

#[derive(Clone)]
pub struct Net {
    pub http: reqwest::Client,
    /// Proxy used for WebSocket tunnels (resolved from config or environment).
    pub ws_proxy: Option<url::Url>,
}

impl Net {
    pub fn new(cfg: &NetConfig) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .use_preconfigured_tls((*tls::client_config()).clone())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60));
        if let Some(p) = &cfg.proxy {
            builder = builder.proxy(reqwest::Proxy::all(p.as_str())?);
        }
        let http = builder.build()?;
        let ws_proxy = cfg
            .proxy
            .clone()
            .or_else(env_proxy)
            .and_then(|p| url::Url::parse(&p).ok())
            .filter(|u| u.scheme() == "http" || u.scheme() == "https");
        Ok(Self { http, ws_proxy })
    }
}

fn env_proxy() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
}

/// Exponential backoff with full jitter.
#[derive(Debug, Clone)]
pub struct Backoff {
    base: Duration,
    max: Duration,
    attempt: u32,
}

impl Backoff {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            base,
            max,
            attempt: 0,
        }
    }

    pub fn next_delay(&mut self) -> Duration {
        let exp = self.base.saturating_mul(1u32 << self.attempt.min(16));
        self.attempt = self.attempt.saturating_add(1);
        let cap = exp.min(self.max);
        let jitter = rand::random::<f64>() * 0.5 + 0.5; // 50%..100%
        cap.mul_f64(jitter)
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn attempts(&self) -> u32 {
        self.attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_bounded() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(10));
        for _ in 0..40 {
            assert!(b.next_delay() <= Duration::from_secs(10));
        }
    }
}

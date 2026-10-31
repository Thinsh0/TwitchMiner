//! Protections in front of every route, for an API that may be reached by a browser
//! (DNS rebinding, cross-site requests) or by strangers (token guessing):
//!
//! - `Host` must be an IP address, a local name (`localhost`, single label, `*.local`,
//!   `*.ts.net`…) or listed in `engine.api_allowed_hosts`;
//! - a request with side effects (anything but `GET`/`HEAD`, and WebSocket upgrades)
//!   carrying an `Origin` must come from the same host or a listed one; native clients
//!   send no `Origin`;
//! - CORS headers only for those origins (no more `*`);
//! - repeated bad tokens from one address lock it out for a while ([`Strikes`]).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use tm_core::settings::{EngineSettings, host_listed};
use tm_core::twitch::netguard;

use crate::AppState;

/// The client's address, when known (inserted by [`guard`]).
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub Option<IpAddr>);

/// Bad tokens tolerated from one address before it is locked out.
pub const FREE_FAILURES: u32 = 10;
/// First lockout; each further one doubles, up to [`MAX_LOCK`].
pub const FIRST_LOCK: Duration = Duration::from_secs(30);
pub const MAX_LOCK: Duration = Duration::from_secs(15 * 60);
/// An address quiet for this long starts again from zero.
const FORGET_AFTER: Duration = Duration::from_secs(60 * 60);
/// Addresses remembered at most (the oldest are forgotten first).
const MAX_TRACKED: usize = 10_000;

#[derive(Debug, Clone, Copy)]
struct Strike {
    failures: u32,
    locks: u32,
    until: Option<Instant>,
    last: Instant,
}

/// Failed authentications per client address. Loopback clients (the apps and `ctl`
/// on the same machine) are never locked out.
#[derive(Debug, Default)]
pub struct Strikes(Mutex<HashMap<IpAddr, Strike>>);

fn exempt(ip: Option<IpAddr>) -> Option<IpAddr> {
    ip.map(|i| i.to_canonical()).filter(|i| !i.is_loopback())
}

impl Strikes {
    /// How long `ip` is still locked out.
    pub fn locked(&self, ip: Option<IpAddr>) -> Option<Duration> {
        let ip = exempt(ip)?;
        let all = self.0.lock().ok()?;
        all.get(&ip)?
            .until?
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
    }

    pub fn failure(&self, ip: Option<IpAddr>) {
        let Some(ip) = exempt(ip) else { return };
        let Ok(mut all) = self.0.lock() else { return };
        let now = Instant::now();
        if all.len() >= MAX_TRACKED {
            all.retain(|_, s| now.duration_since(s.last) < FORGET_AFTER);
            if all.len() >= MAX_TRACKED
                && let Some(oldest) = all.iter().min_by_key(|(_, s)| s.last).map(|(k, _)| *k)
            {
                all.remove(&oldest);
            }
        }
        let s = all.entry(ip).or_insert(Strike {
            failures: 0,
            locks: 0,
            until: None,
            last: now,
        });
        if now.duration_since(s.last) > FORGET_AFTER {
            *s = Strike {
                failures: 0,
                locks: 0,
                until: None,
                last: now,
            };
        }
        s.last = now;
        s.failures += 1;
        if s.failures >= FREE_FAILURES {
            s.failures = 0;
            s.locks = s.locks.saturating_add(1);
            let lock = FIRST_LOCK
                .saturating_mul(1 << (s.locks - 1).min(16))
                .min(MAX_LOCK);
            s.until = Some(now + lock);
            tracing::warn!(%ip, seconds = lock.as_secs(), "too many bad API tokens from this address: locked out");
        }
    }

    pub fn success(&self, ip: Option<IpAddr>) {
        if let (Some(ip), Ok(mut all)) = (exempt(ip), self.0.lock()) {
            all.remove(&ip);
        }
    }
}

/// `host[:port]` (or `[v6][:port]`) → host without brackets, port.
pub fn split_host(h: &str) -> (&str, Option<u16>) {
    let h = h.trim();
    if let Some(rest) = h.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((ip, tail)) => (ip, tail.strip_prefix(':').and_then(|p| p.parse().ok())),
            None => (rest, None),
        };
    }
    match h.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, port.parse().ok()),
        _ => (h, None),
    }
}

/// Whether the API answers to this `Host`: an IP address, a local name, or a name
/// listed in `engine.api_allowed_hosts` (a public name pointing here could be a DNS
/// rebinding attack from a web page).
pub fn host_allowed(host_header: &str, allowed: &[String]) -> bool {
    let (host, _) = split_host(host_header);
    host.parse::<IpAddr>().is_ok() || netguard::is_local_name(host) || host_listed(host, allowed)
}

/// Whether a web page of `origin` may act on this server reached as `host_header`:
/// same host and port, or a listed name.
pub fn origin_allowed(origin: &str, host_header: Option<&str>, allowed: &[String]) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let Some(ohost) = url.host_str() else {
        return false;
    };
    let ohost = ohost.trim_start_matches('[').trim_end_matches(']');
    if host_listed(ohost, allowed) {
        return true;
    }
    let Some((host, port)) = host_header.map(split_host) else {
        return false;
    };
    ohost.eq_ignore_ascii_case(host)
        && match port {
            Some(p) => url.port_or_known_default() == Some(p),
            None => url.port().is_none(),
        }
}

fn refuse(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"ok": false, "error": {"code": code, "message": message}})),
    )
        .into_response()
}

fn is_ws_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

fn add_cors(r: &mut Response, origin: &HeaderValue) {
    let h = r.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    h.insert(header::VARY, HeaderValue::from_static("Origin"));
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
}

/// The middleware in front of every route (see the module documentation).
pub async fn guard(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    req.extensions_mut().insert(ClientIp(ip));
    let allowed = st
        .engine
        .store()
        .read(EngineSettings::from_state)
        .api_allowed_hosts;
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| req.uri().authority().map(|a| a.to_string()));
    if let Some(h) = &host
        && !host_allowed(h, &allowed)
    {
        return refuse(
            StatusCode::FORBIDDEN,
            "forbidden",
            "unknown host name: add it to engine.api_allowed_hosts",
        );
    }
    let origin = req.headers().get(header::ORIGIN).cloned();
    let origin_ok = origin.as_ref().map(|o| {
        o.to_str()
            .is_ok_and(|o| origin_allowed(o, host.as_deref(), &allowed))
    });
    if req.method() == Method::OPTIONS {
        return match (origin, origin_ok) {
            (Some(o), Some(true)) => {
                let mut r = StatusCode::NO_CONTENT.into_response();
                add_cors(&mut r, &o);
                r
            }
            _ => StatusCode::FORBIDDEN.into_response(),
        };
    }
    let side_effects =
        !matches!(*req.method(), Method::GET | Method::HEAD) || is_ws_upgrade(req.headers());
    if side_effects && origin_ok == Some(false) {
        return refuse(
            StatusCode::FORBIDDEN,
            "forbidden",
            "request from a web page of another site refused",
        );
    }
    let mut r = next.run(req).await;
    if let (Some(o), Some(true)) = (origin, origin_ok) {
        add_cors(&mut r, &o);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        let none: Vec<String> = Vec::new();
        for ok in [
            "127.0.0.1:7878",
            "100.64.0.2:7878",
            "[::1]:7878",
            "[fd7a:115c:a1e0::1]",
            "localhost:7878",
            "mini-pc:7878",
            "mini-pc.tail1234.ts.net",
            "raspberrypi.local:7878",
            "twitchminer",
        ] {
            assert!(host_allowed(ok, &none), "{ok}");
        }
        for bad in ["evil.example:7878", "attacker.com", "127.0.0.1.nip.io:7878"] {
            assert!(!host_allowed(bad, &none), "{bad}");
        }
        let list = vec!["tm.example.com".to_string()];
        assert!(host_allowed("tm.example.com", &list));
        assert!(host_allowed("TM.example.com:443", &list));
    }

    #[test]
    fn origins() {
        let none: Vec<String> = Vec::new();
        let h = Some("100.64.0.2:7878");
        assert!(origin_allowed("http://100.64.0.2:7878", h, &none));
        assert!(!origin_allowed("http://100.64.0.2:7879", h, &none));
        assert!(!origin_allowed("http://evil.example", h, &none));
        assert!(!origin_allowed("null", h, &none));
        assert!(origin_allowed(
            "https://tm.example.com",
            Some("tm.example.com"),
            &none
        ));
        assert!(!origin_allowed(
            "https://tm.example.com:8443",
            Some("tm.example.com"),
            &none
        ));
        assert!(origin_allowed(
            "http://[::1]:7878",
            Some("[::1]:7878"),
            &none
        ));
        let list = vec!["ui.example.com".to_string()];
        assert!(origin_allowed("https://ui.example.com", h, &list));
    }

    #[test]
    fn lockout_grows_and_spares_loopback() {
        let s = Strikes::default();
        let ip: Option<IpAddr> = Some("100.64.0.9".parse().unwrap());
        for _ in 0..FREE_FAILURES - 1 {
            s.failure(ip);
        }
        assert!(s.locked(ip).is_none());
        s.failure(ip);
        let first = s.locked(ip).unwrap();
        assert!(first <= FIRST_LOCK && first > FIRST_LOCK / 2);
        for _ in 0..FREE_FAILURES {
            s.failure(ip);
        }
        assert!(s.locked(ip).unwrap() > FIRST_LOCK);
        s.success(ip);
        assert!(s.locked(ip).is_none());
        let local: Option<IpAddr> = Some("127.0.0.1".parse().unwrap());
        let mapped: Option<IpAddr> = Some("::ffff:127.0.0.1".parse().unwrap());
        for _ in 0..100 {
            s.failure(local);
            s.failure(mapped);
            s.failure(None);
        }
        assert!(s.locked(local).is_none() && s.locked(mapped).is_none());
    }
}

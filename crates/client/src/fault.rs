//! Turns a raw connection error into something a person can act on.

/// Why a remote engine cannot be reached, guessed from the connection error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// `https://` against the daemon, which only speaks plain HTTP.
    NotHttps,
    /// The daemon answered but refused the token.
    BadToken,
    /// Nothing listens on that port (engine stopped, wrong port).
    Refused,
    /// The host name does not resolve (typo, Tailscale/VPN not connected).
    UnknownHost,
    /// No answer at all (machine off, firewall, network down).
    NoAnswer,
    /// The address itself is not usable.
    BadUrl,
    /// Anything else: show the raw error.
    Other,
}

/// Classifies `reason` (a `Link::Disconnected` text) for the engine at `url`.
pub fn diagnose(url: &str, reason: &str) -> Fault {
    let r = reason.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| r.contains(n));
    if url.trim().to_ascii_lowercase().starts_with("https:")
        && has(&[
            "invalidcontenttype",
            "corrupt message",
            "wrong version number",
        ])
    {
        Fault::NotHttps
    } else if has(&[
        "401",
        "403",
        "unauthorized",
        "forbidden",
        // Locked out after repeated bad tokens.
        "429",
        "too many requests",
    ]) {
        Fault::BadToken
    } else if has(&[
        "connection refused",
        "actively refused",
        "os error 111",
        "os error 10061",
    ]) {
        Fault::Refused
    } else if has(&[
        "failed to lookup address",
        "name or service not known",
        "no such host",
        "nodename nor servname",
        "temporary failure in name resolution",
        "os error 11001",
    ]) {
        Fault::UnknownHost
    } else if has(&[
        "no answer",
        "timed out",
        "network is unreachable",
        "no route to host",
        "host is down",
        "os error 110",
        "os error 10060",
        "os error 10065",
    ]) {
        Fault::NoAnswer
    } else if has(&["relative url", "url error", "invalid uri", "empty host"]) {
        Fault::BadUrl
    } else {
        Fault::Other
    }
}

/// A token as pasted: copying it from a terminal or a log often brings spaces or line
/// breaks (tokens never contain any).
pub fn clean_token(token: &str) -> String {
    token.chars().filter(|c| !c.is_whitespace()).collect()
}

/// `192.168.1.10:7878` → `http://192.168.1.10:7878`; trailing slashes removed.
pub fn normalize_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.contains("://") {
        url.to_string()
    } else {
        format!("http://{url}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_errors() {
        let cases = [
            (
                "https://h:7878",
                "websocket error: IO error: received corrupt message of type InvalidContentType",
                Fault::NotHttps,
            ),
            (
                "http://h:7878",
                "websocket error: HTTP error: 401 Unauthorized",
                Fault::BadToken,
            ),
            (
                "http://h:7878",
                "websocket error: HTTP error: 429 Too Many Requests",
                Fault::BadToken,
            ),
            (
                "http://h:7878",
                "network error: Connection refused (os error 111)",
                Fault::Refused,
            ),
            (
                "http://h:7878",
                "network error: No connection could be made because the target machine actively refused it. (os error 10061)",
                Fault::Refused,
            ),
            (
                "http://h:7878",
                "network error: failed to lookup address information: Name or service not known",
                Fault::UnknownHost,
            ),
            (
                "http://h:7878",
                "network error: No such host is known. (os error 11001)",
                Fault::UnknownHost,
            ),
            ("http://h:7878", "no answer", Fault::NoAnswer),
            (
                "h:7878",
                "websocket error: relative URL without a base",
                Fault::BadUrl,
            ),
            ("http://h:7878", "something new", Fault::Other),
        ];
        for (url, reason, fault) in cases {
            assert_eq!(diagnose(url, reason), fault, "{reason}");
        }
    }

    #[test]
    fn tokens_survive_copy_and_paste() {
        assert_eq!(clean_token(" ab12\r\ncd34 \u{a0}ef "), "ab12cd34ef");
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize_url(" 10.0.0.2:7878/ "), "http://10.0.0.2:7878");
        assert_eq!(normalize_url("http://h:7878/"), "http://h:7878");
        assert_eq!(normalize_url("https://h"), "https://h");
    }
}

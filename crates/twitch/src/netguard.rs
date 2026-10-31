//! Outbound requests to addresses chosen by someone other than the owner (e.g. a
//! guest's notification webhook): `https` only, public hosts only, checked again
//! after DNS resolution (see [`PublicResolver`]) and never following redirects
//! (see `Net::guarded`). Also used to recognise local host names (`is_local_name`).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Why an address is refused. Deliberately vague: the reason must not help someone
/// map the server's network.
pub const REFUSED: &str = "only public https:// addresses are allowed";

/// `true` for an address reachable on the public Internet: not loopback, private,
/// link-local, CGNAT (Tailscale's 100.64.0.0/10), unique local (fc00::/7), multicast,
/// documentation or reserved, including IPv4 hidden in IPv6 forms.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        // 100.64.0.0/10: carrier-grade NAT, Tailscale.
        || (o[0] == 100 && (o[1] & 0xc0) == 64)
        // 192.0.0.0/24: IETF protocol assignments.
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        // 198.18.0.0/15: benchmarking.
        || (o[0] == 198 && (o[1] & 0xfe) == 18)
        // 240.0.0.0/4: reserved.
        || o[0] >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        // fc00::/7: unique local (Tailscale uses fd7a:115c:a1e0::/48).
        || (s[0] & 0xfe00) == 0xfc00
        // fe80::/10 link-local, fec0::/10 site-local.
        || (s[0] & 0xffc0) == 0xfe80
        || (s[0] & 0xffc0) == 0xfec0
        // 2001:db8::/32 documentation, 2001::/32 Teredo, 2002::/16 6to4,
        // 64:ff9b::/96 NAT64: may lead to any IPv4 address.
        || (s[0] == 0x2001 && (s[1] == 0x0db8 || s[1] == 0))
        || s[0] == 0x2002
        || (s[0] == 0x0064 && s[1] == 0xff9b)
        // ::/96: deprecated IPv4-compatible addresses.
        || s[..6].iter().all(|&x| x == 0))
}

/// Suffixes that only exist on a local network (never resolvable from the Internet,
/// so never a DNS rebinding vector): mDNS, home routers, Docker, Tailscale MagicDNS.
const LOCAL_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "localdomain",
    "lan",
    "home",
    "home.arpa",
    "internal",
    "intranet",
    "corp",
    "ts.net",
];

/// `true` for a host name that designates a machine of the local network: a single
/// label (`mini-pc`, a Docker service) or a local suffix (`*.local`, `*.ts.net`…).
pub fn is_local_name(name: &str) -> bool {
    let n = name.trim_end_matches('.').to_ascii_lowercase();
    !n.contains('.')
        || LOCAL_SUFFIXES
            .iter()
            .any(|s| n == *s || n.strip_suffix(s).is_some_and(|p| p.ends_with('.')))
}

/// Checks an address given by someone other than the owner, without any network
/// access: `https`, no credentials, a public IP or a non-local host name.
pub fn check_public_url(raw: &str) -> Result<url::Url, &'static str> {
    let url = url::Url::parse(raw.trim()).map_err(|_| REFUSED)?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(REFUSED);
    }
    let ok = match url.host() {
        Some(url::Host::Ipv4(ip)) => is_public_ip(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_public_ip(IpAddr::V6(ip)),
        Some(url::Host::Domain(d)) => !is_local_name(d),
        None => false,
    };
    if ok { Ok(url) } else { Err(REFUSED) }
}

/// Resolves `host` and accepts it only when every address is public (a name that
/// also resolves to a private address is refused as a whole).
pub async fn resolve_public(host: &str, port: u16) -> Result<Vec<SocketAddr>, &'static str> {
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        return if is_public_ip(ip) {
            Ok(vec![SocketAddr::new(ip, port)])
        } else {
            Err(REFUSED)
        };
    }
    if is_local_name(host) {
        return Err(REFUSED);
    }
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| REFUSED)?
        .collect();
    if addrs.is_empty() || addrs.iter().any(|a| !is_public_ip(a.ip())) {
        return Err(REFUSED);
    }
    Ok(addrs)
}

/// [`check_public_url`], then [`resolve_public`] on its host. Needed when requests go
/// through a proxy, which resolves names itself (`PublicResolver` is then not used).
pub async fn check_public_url_resolved(raw: &str) -> Result<url::Url, &'static str> {
    let url = check_public_url(raw)?;
    let host = url.host_str().ok_or(REFUSED)?;
    resolve_public(host, url.port_or_known_default().unwrap_or(443)).await?;
    Ok(url)
}

/// DNS resolver that refuses names resolving to non-public addresses: the check
/// happens at connection time, so a name cannot be re-pointed after validation.
#[derive(Debug, Default, Clone, Copy)]
pub struct PublicResolver;

impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs = resolve_public(&host, 0).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                Box::new(addrs.into_iter()) as reqwest::dns::Addrs
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.10",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.254",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fd7a:115c:a1e0::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "2002:a00:1::",
            "::127.0.0.1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "100.128.0.1", "8.8.8.8", "2606:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn local_names() {
        for n in [
            "localhost",
            "mini-pc",
            "a.localhost",
            "raspberrypi.local",
            "nas.lan",
            "box.home.arpa",
            "host.docker.internal",
            "mini-pc.tail1234.ts.net",
            "ntfy.",
        ] {
            assert!(is_local_name(n), "{n}");
        }
        for n in ["ntfy.sh", "discord.com", "notlocal.com", "evil-ts.net"] {
            assert!(!is_local_name(n), "{n}");
        }
    }

    #[test]
    fn urls_for_guests() {
        assert!(check_public_url("https://ntfy.sh/topic").is_ok());
        assert!(check_public_url("https://discord.com/api/webhooks/1/x").is_ok());
        assert!(check_public_url("https://1.1.1.1:8443/x").is_ok());
        for bad in [
            "http://ntfy.sh/topic",
            "https://127.0.0.1/x",
            "https://2130706433/x",
            "https://0x7f.1/x",
            "https://[::1]/x",
            "https://192.168.1.1/x",
            "https://100.100.100.100/x",
            "https://localhost/x",
            "https://mini-pc:7878/x",
            "https://nas.local/x",
            "https://user:pass@ntfy.sh/x",
            "ftp://ntfy.sh/x",
            "file:///etc/passwd",
            "not a url",
        ] {
            assert!(check_public_url(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn resolution_is_checked() {
        assert!(resolve_public("127.0.0.1", 80).await.is_err());
        assert!(resolve_public("localhost", 80).await.is_err());
        assert!(resolve_public("[::1]", 80).await.is_err());
        assert!(resolve_public("1.1.1.1", 443).await.is_ok());
    }
}

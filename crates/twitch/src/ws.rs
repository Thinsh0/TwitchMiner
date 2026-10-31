//! WebSocket connection helper with HTTP CONNECT proxy support
//! (tungstenite does not do proxies on its own).

use std::time::Duration;

use base64::Engine;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream, client_async_tls_with_config};

use crate::error::{Error, Result};
use crate::net::Net;
use crate::tls;

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub async fn connect(net: &Net, url: &str) -> Result<WsStream> {
    connect_with_token(net, url, None).await
}

/// Like [`connect`], sending `Authorization: Bearer <token>` with the upgrade request
/// (keeps the token out of the URL, hence out of logs).
pub async fn connect_with_token(net: &Net, url: &str, token: Option<&str>) -> Result<WsStream> {
    tokio::time::timeout(CONNECT_TIMEOUT, connect_inner(net, url, token))
        .await
        .map_err(|_| Error::Timeout(CONNECT_TIMEOUT))?
}

async fn connect_inner(net: &Net, url: &str, token: Option<&str>) -> Result<WsStream> {
    let target = url::Url::parse(url).map_err(|e| Error::WebSocket(e.to_string()))?;
    let host = target
        .host_str()
        .ok_or_else(|| Error::WebSocket("no host".into()))?
        .to_string();
    let port = target.port_or_known_default().unwrap_or(443);

    let tcp = match &net.ws_proxy {
        Some(proxy) if !bypass_proxy(&host) => tunnel(proxy, &host, port).await?,
        _ => TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|e| Error::Network(e.to_string()))?,
    };
    tcp.set_nodelay(true).ok();
    let connector = Connector::Rustls(tls::client_config());
    let mut request = url
        .into_client_request()
        .map_err(|e| Error::WebSocket(e.to_string()))?;
    if let Some(t) = token {
        let value = HeaderValue::from_str(&format!("Bearer {t}"))
            .map_err(|_| Error::WebSocket("token not usable in a header".into()))?;
        request.headers_mut().insert(
            tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
            value,
        );
    }
    let (ws, _resp) = client_async_tls_with_config(request, tcp, None, Some(connector))
        .await
        .map_err(|e| Error::WebSocket(e.to_string()))?;
    Ok(ws)
}

fn bypass_proxy(host: &str) -> bool {
    let no_proxy = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default();
    no_proxy
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .any(|p| {
            let p = p.trim_start_matches('*').trim_start_matches('.');
            host == p || host.ends_with(&format!(".{p}"))
        })
}

async fn tunnel(proxy: &url::Url, host: &str, port: u16) -> Result<TcpStream> {
    let phost = proxy
        .host_str()
        .ok_or_else(|| Error::Network("bad proxy url".into()))?;
    let pport = proxy.port_or_known_default().unwrap_or(8080);
    let mut stream = TcpStream::connect((phost, pport))
        .await
        .map_err(|e| Error::Network(format!("proxy: {e}")))?;
    let mut req = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
    if !proxy.username().is_empty() {
        let creds = format!("{}:{}", proxy.username(), proxy.password().unwrap_or(""));
        let b64 = base64::engine::general_purpose::STANDARD.encode(creds);
        req.push_str(&format!("Proxy-Authorization: Basic {b64}\r\n"));
    }
    req.push_str("\r\n");
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| Error::Network(e.to_string()))?;

    // Read the response head byte-wise so no tunnel bytes are consumed.
    let mut reader = BufReader::with_capacity(1, &mut stream);
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .await
        .map_err(|e| Error::Network(e.to_string()))?;
    let ok = status_line
        .split_whitespace()
        .nth(1)
        .is_some_and(|c| c == "200");
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|e| Error::Network(e.to_string()))?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }
    if !ok {
        return Err(Error::Network(format!(
            "proxy CONNECT refused: {}",
            status_line.trim()
        )));
    }
    Ok(stream)
}

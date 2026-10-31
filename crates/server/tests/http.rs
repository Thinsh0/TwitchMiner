//! HTTP and WebSocket protections against a real server (no Twitch account needed):
//! Content-Type, Origin, Host, CORS, WebSocket authentication, no token off loopback.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tm_core::{Engine, EngineConfig, Paths};
use tm_server::ServerConfig;
use tokio_tungstenite::tungstenite::Message;

const TOKEN: &str = "owner-token";

async fn engine() -> Engine {
    tm_twitch::tls::install_crypto_provider();
    let dir = std::env::temp_dir().join(format!("tm-test-{}", uuid::Uuid::new_v4()));
    Engine::start(EngineConfig {
        paths: Paths::at(dir).unwrap(),
        modules: Vec::new(),
        version: "test".into(),
    })
    .await
    .unwrap()
}

/// A server on 127.0.0.1 with the owner token; returns its address.
async fn serve(engine: &Engine) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = ServerConfig {
        bind: addr,
        token: Some(TOKEN.into()),
        agent_token: None,
        logs: None,
    };
    tokio::spawn(tm_server::serve_on(
        listener,
        engine.clone(),
        cfg,
        std::future::pending(),
    ));
    addr
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn net() -> tm_twitch::Net {
    let net = tm_twitch::Net::new(&tm_twitch::NetConfig::default()).unwrap();
    tm_twitch::Net {
        ws_proxy: None,
        ..net
    }
}

#[tokio::test]
async fn commands_need_json() {
    let engine = engine().await;
    let addr = serve(&engine).await;
    let url = format!("http://{addr}/api/v1/command");
    let body = r#"{"cmd":"summary"}"#;
    let plain = http()
        .post(&url)
        .bearer_auth(TOKEN)
        .header("Content-Type", "text/plain")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(plain.status(), 415);
    let none = http()
        .post(&url)
        .bearer_auth(TOKEN)
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(none.status(), 415);
    let ok = http()
        .post(&url)
        .bearer_auth(TOKEN)
        .header("Content-Type", "application/json; charset=utf-8")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    engine.shutdown().await;
}

#[tokio::test]
async fn other_sites_and_unknown_hosts_are_refused() {
    let engine = engine().await;
    let addr = serve(&engine).await;
    let url = format!("http://{addr}/api/v1/command");
    let post = |origin: Option<&str>| {
        let mut r = http()
            .post(&url)
            .bearer_auth(TOKEN)
            .json(&json!({"cmd": "summary"}));
        if let Some(o) = origin {
            r = r.header("Origin", o);
        }
        r.send()
    };
    // Native clients send no Origin; the built-in page sends its own.
    assert_eq!(post(None).await.unwrap().status(), 200);
    let same = post(Some(&format!("http://{addr}"))).await.unwrap();
    assert_eq!(same.status(), 200);
    assert_eq!(
        same.headers()["access-control-allow-origin"],
        format!("http://{addr}").as_str()
    );
    assert_eq!(
        post(Some("http://evil.example")).await.unwrap().status(),
        403
    );
    assert_eq!(post(Some("null")).await.unwrap().status(), 403);

    // No `*` any more; preflights only for allowed origins.
    let state = http()
        .get(format!("http://{addr}/api/v1/state"))
        .bearer_auth(TOKEN)
        .header("Origin", "http://evil.example")
        .send()
        .await
        .unwrap();
    assert!(state.headers().get("access-control-allow-origin").is_none());
    let preflight = |origin: &str| {
        http()
            .request(reqwest::Method::OPTIONS, &url)
            .header("Origin", origin)
            .header("Access-Control-Request-Method", "POST")
            .send()
    };
    assert_eq!(
        preflight("http://evil.example").await.unwrap().status(),
        403
    );
    assert_eq!(
        preflight(&format!("http://{addr}")).await.unwrap().status(),
        204
    );

    // DNS rebinding: a public name pointing here is refused, unless listed.
    let with_host = |host: &str| {
        http()
            .get(format!("http://{addr}/api/v1/health"))
            .header("Host", host)
            .send()
    };
    assert_eq!(with_host("evil.example:7878").await.unwrap().status(), 403);
    assert_eq!(with_host("mini-pc:7878").await.unwrap().status(), 200);
    assert_eq!(
        with_host("mini-pc.tail1234.ts.net").await.unwrap().status(),
        200
    );
    assert_eq!(with_host("100.64.0.2:7878").await.unwrap().status(), 200);
    engine
        .execute(tm_core::Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"api_allowed_hosts": ["tm.example.com"]}),
            merge: true,
        })
        .await
        .unwrap();
    assert_eq!(with_host("tm.example.com").await.unwrap().status(), 200);
    assert_eq!(with_host("evil.example").await.unwrap().status(), 403);
    engine.shutdown().await;
}

#[tokio::test]
async fn no_token_only_on_loopback() {
    let engine = engine().await;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let cfg = ServerConfig {
        bind: listener.local_addr().unwrap(),
        token: None,
        agent_token: None,
        logs: None,
    };
    let r = tm_server::serve_on(listener, engine.clone(), cfg, async {}).await;
    assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
    // On loopback it is accepted.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = ServerConfig {
        bind: listener.local_addr().unwrap(),
        token: None,
        agent_token: None,
        logs: None,
    };
    assert!(
        tm_server::serve_on(listener, engine.clone(), cfg, async {})
            .await
            .is_ok()
    );
    engine.shutdown().await;
}

async fn first_text(ws: &mut tm_twitch::ws::WsStream) -> Value {
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
            .await
            .expect("an answer")
        {
            Some(Ok(Message::Text(t))) => return serde_json::from_str(&t).unwrap(),
            Some(Ok(Message::Close(_))) | None => return Value::Null,
            Some(Ok(_)) => continue,
            Some(Err(e)) => panic!("{e}"),
        }
    }
}

#[tokio::test]
async fn websocket_authentication() {
    let engine = engine().await;
    let addr = serve(&engine).await;
    let url = format!("ws://{addr}/api/v1/ws");
    let net = net();

    // Header (tm-client), without the token in the URL.
    let mut ws = tm_twitch::ws::connect_with_token(&net, &url, Some(TOKEN))
        .await
        .unwrap();
    assert_eq!(first_text(&mut ws).await["type"], "snapshot");

    // First message (the built-in page).
    let mut ws = tm_twitch::ws::connect(&net, &url).await.unwrap();
    ws.send(Message::text(
        json!({"type": "auth", "token": TOKEN}).to_string(),
    ))
    .await
    .unwrap();
    let snap = first_text(&mut ws).await;
    assert_eq!(
        (snap["type"].as_str(), snap["access"].as_str()),
        (Some("snapshot"), Some("full"))
    );
    // Commands work afterwards.
    ws.send(Message::text(
        json!({"id": 1, "cmd": {"cmd": "summary"}}).to_string(),
    ))
    .await
    .unwrap();
    loop {
        let m = first_text(&mut ws).await;
        if m["type"] == "reply" {
            assert_eq!(m["ok"], true);
            break;
        }
    }

    // A bad first message: told why, then closed.
    let mut ws = tm_twitch::ws::connect(&net, &url).await.unwrap();
    ws.send(Message::text(
        json!({"type": "auth", "token": "nope"}).to_string(),
    ))
    .await
    .unwrap();
    let err = first_text(&mut ws).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["error"]["code"], "unauthorized");
    assert_eq!(first_text(&mut ws).await, Value::Null);

    // A command before authenticating is not run.
    let mut ws = tm_twitch::ws::connect(&net, &url).await.unwrap();
    ws.send(Message::text(
        json!({"id": 1, "cmd": {"cmd": "snapshot"}}).to_string(),
    ))
    .await
    .unwrap();
    assert_eq!(first_text(&mut ws).await["type"], "error");

    // Deprecated, still accepted: the token in the URL.
    let mut ws = tm_twitch::ws::connect(&net, &format!("{url}?token={TOKEN}"))
        .await
        .unwrap();
    assert_eq!(first_text(&mut ws).await["type"], "snapshot");
    // A bad token with the request: refused at once.
    assert!(
        tm_twitch::ws::connect_with_token(&net, &url, Some("nope"))
            .await
            .unwrap_err()
            .to_string()
            .contains("401")
    );

    // A WebSocket opened by a web page of another site.
    let mut req = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
        url.as_str(),
    )
    .unwrap();
    req.headers_mut()
        .insert("Origin", "http://evil.example".parse().unwrap());
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let refused = tokio_tungstenite::client_async(req, tcp).await;
    assert!(refused.unwrap_err().to_string().contains("403"));
    engine.shutdown().await;
}

/// Repeated bad tokens lock a (non-loopback) address out. Needs a local address
/// other than loopback; skipped without one.
#[tokio::test]
async fn bad_tokens_lock_the_address_out() {
    let Some(ip) = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").map(|_| s))
        .and_then(|s| s.local_addr())
        .ok()
        .map(|a| a.ip())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
    else {
        eprintln!("no non-loopback address: skipped");
        return;
    };
    let engine = engine().await;
    let Ok(listener) = tokio::net::TcpListener::bind((ip, 0)).await else {
        eprintln!("cannot listen on {ip}: skipped");
        return;
    };
    let addr = listener.local_addr().unwrap();
    let cfg = ServerConfig {
        bind: addr,
        token: Some(TOKEN.into()),
        agent_token: None,
        logs: None,
    };
    tokio::spawn(tm_server::serve_on(
        listener,
        engine.clone(),
        cfg,
        std::future::pending(),
    ));
    let get = |tok: &str| {
        http()
            .get(format!("http://{addr}/api/v1/state"))
            .bearer_auth(tok)
            .send()
    };
    assert_eq!(get(TOKEN).await.unwrap().status(), 200);
    for _ in 0..tm_server::FREE_FAILURES {
        assert_eq!(get("nope").await.unwrap().status(), 401);
    }
    // Locked: even the right token waits.
    let locked = get(TOKEN).await.unwrap();
    assert_eq!(locked.status(), 429);
    assert!(locked.headers().get("retry-after").is_some());
    engine.shutdown().await;
}

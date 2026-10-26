//! A guest token against a real engine and server (no Twitch account needed).

use serde_json::{Value, json};
use tm_core::{Command, Engine, EngineConfig, Paths};
use tm_server::access::Access;
use tm_server::{ServerConfig, run_command};

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

async fn invite(engine: &Engine) -> (String, String) {
    let out = run_command(
        engine,
        Access::Full,
        Command::GuestInvite { name: "Sam".into() },
    )
    .await
    .unwrap();
    (
        out["id"].as_str().unwrap().to_string(),
        out["token"].as_str().unwrap().to_string(),
    )
}

#[tokio::test]
async fn guests_are_kept_apart() {
    let engine = engine().await;
    let (id, token) = invite(&engine).await;
    let guest = Access::Guest(id.clone());

    // Only the token's hash is stored.
    let state = engine.store().snapshot();
    let doc = state.settings["guests"].to_string();
    assert!(doc.contains("Sam") && !doc.contains(&token));

    // Owner-only commands and scopes.
    for cmd in [
        Command::GuestInvite { name: "x".into() },
        Command::GuestRemove { id: id.clone() },
        Command::Backup { path: None },
        Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"paused": true}),
            merge: true,
        },
        Command::SettingsSet {
            scope: "module:drops".into(),
            value: json!({}),
            merge: true,
        },
        Command::SettingsSet {
            scope: "guests".into(),
            value: json!({}),
            merge: true,
        },
    ] {
        let r = run_command(&engine, guest.clone(), cmd).await;
        assert_eq!(r.unwrap_err().code, "forbidden");
    }
    for cmd in [
        Command::GuestInvite { name: "x".into() },
        Command::SettingsGet {
            scope: "guests".into(),
            account: None,
        },
    ] {
        let r = run_command(&engine, Access::Agent, cmd).await;
        assert_eq!(r.unwrap_err().code, "forbidden");
    }

    // The snapshot holds neither the guests nor other secrets.
    run_command(
        &engine,
        Access::Full,
        Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"proxy": "http://u:secret@127.0.0.1:9"}),
            merge: true,
        },
    )
    .await
    .unwrap();
    let snap = run_command(&engine, guest.clone(), Command::Snapshot)
        .await
        .unwrap()
        .to_string();
    assert!(!snap.contains("token_sha256") && !snap.contains("secret"));

    // Revoked: the token stops working.
    run_command(
        &engine,
        Access::Full,
        Command::GuestRemove { id: id.clone() },
    )
    .await
    .unwrap();
    let r = run_command(&engine, guest, Command::Snapshot).await;
    assert_eq!(r.unwrap_err().code, "unauthorized");
    engine.shutdown().await;
}

#[tokio::test]
async fn guest_token_over_http() {
    let engine = engine().await;
    let (_, token) = invite(&engine).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = ServerConfig {
        bind: addr,
        token: Some("owner-token".into()),
        agent_token: None,
        logs: None,
    };
    let server = tokio::spawn(tm_server::serve_on(
        listener,
        engine.clone(),
        cfg,
        std::future::pending(),
    ));
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let get = |path: &str, tok: &str| {
        http.get(format!("http://{addr}/api/v1/{path}"))
            .bearer_auth(tok)
            .send()
    };
    let state: Value = get("state", &token).await.unwrap().json().await.unwrap();
    assert!(state["settings"].get("guests").is_none());
    let full: Value = get("state", "owner-token")
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(full["settings"].get("guests").is_some());
    assert_eq!(get("logs", &token).await.unwrap().status(), 403);
    assert_eq!(get("state", "nope").await.unwrap().status(), 401);
    server.abort();
    engine.shutdown().await;
}

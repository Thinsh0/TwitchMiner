use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tm_client::{Backend, Link, Local, Remote};
use tm_core::{Command, Engine, EngineConfig, Paths, State};

async fn wait_for(backend: &dyn Backend, pred: impl Fn(&State) -> bool) -> Arc<State> {
    let mut rx = backend.state();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let s = rx.borrow_and_update().clone();
            if pred(&s) {
                return s;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("state never matched")
}

#[tokio::test(flavor = "multi_thread")]
async fn local_and_remote_mirror_the_same_state() {
    tm_twitch::tls::install_crypto_provider();
    let dir = std::env::temp_dir().join(format!("tm-client-{}", std::process::id()));
    let engine = Engine::start(EngineConfig {
        paths: Paths::at(&dir).unwrap(),
        modules: vec![],
        version: "test".into(),
    })
    .await
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = tm_server::ServerConfig {
        bind: addr,
        token: Some("secret".into()),
        agent_token: Some("agent".into()),
        logs: None,
    };
    tokio::spawn(tm_server::serve_on(
        listener,
        engine.clone(),
        cfg,
        std::future::pending(),
    ));

    let local = Local::new(engine.clone());
    let remote = Remote::connect(&format!("http://{addr}"), "secret")
        .await
        .unwrap();
    assert_eq!(*remote.link().borrow(), Link::Connected);

    // A command sent through the remote backend…
    remote
        .execute(Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"max_watch_slots": 1}),
            merge: true,
        })
        .await
        .unwrap();
    // …is visible in both mirrors, identical to the engine's own state.
    let has = |s: &State| s.settings.get("engine") == Some(&json!({"max_watch_slots": 1}));
    let l = wait_for(&local, has).await;
    let r = wait_for(&remote, has).await;
    assert_eq!(l.seq, r.seq);
    assert_eq!(*l, engine.store().snapshot());
    assert_eq!(*r, engine.store().snapshot());

    // Logs are reachable remotely too (this test server keeps none).
    assert_eq!(remote.logs(10).await.unwrap().len(), 0);

    // Errors come back typed.
    let bad = remote
        .execute(Command::SettingsSet {
            scope: "bogus".into(),
            value: json!({}),
            merge: false,
        })
        .await
        .unwrap_err();
    assert_eq!(bad.code, "invalid");

    // The agent token is refused until the owner enables agent access…
    let early = Remote::connect(&format!("http://{addr}"), "agent")
        .await
        .unwrap();
    assert_ne!(*early.link().borrow(), Link::Connected);
    remote
        .execute(Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"agent_enabled": true}),
            merge: true,
        })
        .await
        .unwrap();
    // …then works, but cannot change sensitive engine settings.
    let agent = Remote::connect(&format!("http://{addr}"), "agent")
        .await
        .unwrap();
    assert_eq!(*agent.link().borrow(), Link::Connected);
    let denied = agent
        .execute(Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"proxy": "http://evil:1"}),
            merge: true,
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code, "forbidden");
    agent
        .execute(Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"paused": true}),
            merge: true,
        })
        .await
        .unwrap();

    // A token pasted with a line break and spaces still works.
    let pasted = Remote::connect(&format!("http://{addr}"), " sec\r\nret ")
        .await
        .unwrap();
    assert_eq!(*pasted.link().borrow(), Link::Connected);
    drop(pasted);

    // Wrong token is refused.
    let intruder = Remote::connect(&format!("http://{addr}"), "wrong")
        .await
        .unwrap();
    assert_ne!(*intruder.link().borrow(), Link::Connected);
    // Commands are refused at once while the link is down.
    let started = std::time::Instant::now();
    let down = intruder.execute(Command::Snapshot).await.unwrap_err();
    assert_eq!(down.code, "unavailable");
    assert!(started.elapsed() < Duration::from_secs(1));

    // A burst of events ends in the same state on both sides.
    let burst = (0..100).map(|i| {
        let engine = engine.clone();
        tokio::spawn(async move {
            engine
                .execute(Command::SettingsSet {
                    scope: "engine".into(),
                    value: json!({"max_watch_slots": 1 + i % 2}),
                    merge: true,
                })
                .await
        })
    });
    for b in burst.collect::<Vec<_>>() {
        b.await.unwrap().unwrap();
    }
    let seq = engine.store().snapshot().seq;
    let r = wait_for(&remote, |s| s.seq == seq).await;
    assert_eq!(*r, engine.store().snapshot());

    engine.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

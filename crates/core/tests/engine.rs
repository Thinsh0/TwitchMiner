//! Engine-level tests that need no Twitch access (no account is configured).

use std::sync::Arc;

use serde_json::json;
use tm_core::{Command, Engine, EngineConfig, Module, ModuleContext, ModuleInfo, Paths};

struct Dummy;

#[async_trait::async_trait]
impl Module for Dummy {
    fn info(&self) -> ModuleInfo {
        ModuleInfo::new("dummy", "Dummy", "test", json!({ "level": 1 }))
    }
    fn validate_settings(&self, v: &serde_json::Value) -> Result<(), String> {
        match v["level"].as_u64() {
            Some(l) if l <= 10 => Ok(()),
            _ => Err("level must be 0..=10".into()),
        }
    }
    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
        ctx.cancelled().await;
        Ok(())
    }
}

fn temp_paths() -> Paths {
    let dir = std::env::temp_dir().join(format!("tm-test-{}", uuid::Uuid::new_v4()));
    Paths::at(dir).unwrap()
}

async fn start(paths: &Paths) -> Engine {
    Engine::start(EngineConfig {
        paths: paths.clone(),
        modules: vec![Arc::new(Dummy) as Arc<dyn Module>],
        version: "test".into(),
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn settings_are_validated_persisted_and_layered() {
    tm_twitch::tls::install_crypto_provider();
    let paths = temp_paths();
    let engine = start(&paths).await;

    // Engine settings: type-checked.
    let bad = engine
        .execute(Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"max_watch_slots": "two"}),
            merge: true,
        })
        .await;
    assert!(bad.is_err());
    engine
        .execute(Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"max_watch_slots": 1}),
            merge: true,
        })
        .await
        .unwrap();

    // Module settings: validated by the module against the effective document.
    let bad = engine
        .execute(Command::SettingsSet {
            scope: "module:dummy".into(),
            value: json!({"level": 99}),
            merge: true,
        })
        .await;
    assert_eq!(bad.unwrap_err().code, "invalid");
    engine
        .execute(Command::SettingsSet {
            scope: "module:dummy".into(),
            value: json!({"level": 3}),
            merge: true,
        })
        .await
        .unwrap();
    engine
        .execute(Command::SettingsSet {
            scope: "account:42/module:dummy".into(),
            value: json!({"extra": true}),
            merge: true,
        })
        .await
        .unwrap();
    let got = engine
        .execute(Command::SettingsGet {
            scope: "account:42/module:dummy".into(),
            account: None,
        })
        .await
        .unwrap();
    assert_eq!(
        got["effective"],
        json!({"enabled": true, "level": 3, "extra": true})
    );

    let unknown = engine
        .execute(Command::SettingsSet {
            scope: "module:nope".into(),
            value: json!({}),
            merge: false,
        })
        .await;
    assert_eq!(unknown.unwrap_err().code, "not_found");

    let modules = engine.execute(Command::ModulesList).await.unwrap();
    assert_eq!(modules[0]["id"], "dummy");
    engine.shutdown().await;
    drop(engine);

    // Everything is in the single data directory: a restart sees the same settings.
    let engine = start(&paths).await;
    let state = engine.store().snapshot();
    assert_eq!(state.settings["engine"], json!({"max_watch_slots": 1}));
    assert_eq!(state.settings["module:dummy"], json!({"level": 3}));
    assert!(state.accounts.is_empty());

    // Deleting a scope.
    engine
        .execute(Command::SettingsSet {
            scope: "module:dummy".into(),
            value: serde_json::Value::Null,
            merge: false,
        })
        .await
        .unwrap();
    assert!(
        !engine
            .store()
            .snapshot()
            .settings
            .contains_key("module:dummy")
    );
    engine.shutdown().await;
    let _ = std::fs::remove_dir_all(&paths.root);
}

#[tokio::test]
async fn subscribers_see_every_event_in_order() {
    tm_twitch::tls::install_crypto_provider();
    let paths = temp_paths();
    let engine = start(&paths).await;
    let (snap, mut rx) = engine.store().snapshot_and_subscribe();
    for i in 0..5 {
        engine
            .execute(Command::SettingsSet {
                scope: "engine".into(),
                value: json!({"history_retention_days": 10 + i}),
                merge: true,
            })
            .await
            .unwrap();
    }
    // Every event, in order, until the mirror caught up (the engine emits some of its
    // own, e.g. `watching_allowed` at start).
    let mut mirror = snap;
    let target = engine.store().snapshot().seq;
    while mirror.seq < target {
        let env = rx.recv().await.unwrap();
        assert_eq!(env.seq, mirror.seq + 1);
        tm_core::state::apply_envelope(&mut mirror, &env);
    }
    assert_eq!(mirror, engine.store().snapshot());
    engine.shutdown().await;
    let _ = std::fs::remove_dir_all(&paths.root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_settings_merges_are_all_kept() {
    tm_twitch::tls::install_crypto_provider();
    let paths = temp_paths();
    let engine = start(&paths).await;
    let writes = (0..30).map(|i| {
        let engine = engine.clone();
        tokio::spawn(async move {
            engine
                .execute(Command::SettingsSet {
                    scope: "module:dummy".into(),
                    value: json!({ format!("k{i}"): i }),
                    merge: true,
                })
                .await
                .unwrap();
        })
    });
    for w in writes.collect::<Vec<_>>() {
        w.await.unwrap();
    }
    let expected: serde_json::Map<String, serde_json::Value> =
        (0..30).map(|i| (format!("k{i}"), json!(i))).collect();
    let state = engine.store().snapshot();
    assert_eq!(state.settings["module:dummy"], json!(expected));
    engine.shutdown().await;
    drop(engine);

    // The database holds the same document as the state.
    let engine = start(&paths).await;
    assert_eq!(
        engine.store().snapshot().settings["module:dummy"],
        json!(expected)
    );
    engine.shutdown().await;
    let _ = std::fs::remove_dir_all(&paths.root);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_engine_says_whether_watching_is_allowed() {
    tm_twitch::tls::install_crypto_provider();
    let paths = temp_paths();
    let engine = start(&paths).await;
    let wait_for = |want: bool| {
        let engine = engine.clone();
        async move {
            for _ in 0..50 {
                if engine.store().snapshot().watching_allowed == Some(want) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            panic!("watching_allowed never became {want}");
        }
    };
    wait_for(true).await;
    engine
        .execute(Command::SettingsSet {
            scope: "engine".into(),
            value: json!({"paused": true}),
            merge: true,
        })
        .await
        .unwrap();
    wait_for(false).await;
    engine.shutdown().await;
    let _ = std::fs::remove_dir_all(&paths.root);
}

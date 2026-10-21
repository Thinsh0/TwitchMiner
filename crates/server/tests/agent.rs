//! The agent token against a real engine (no Twitch account needed).

use serde_json::{Value, json};
use tm_core::{Command, Engine, EngineConfig, Paths};
use tm_server::access::Access;
use tm_server::run_command;

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

fn set(value: Value, merge: bool) -> Command {
    Command::SettingsSet {
        scope: "engine".into(),
        value,
        merge,
    }
}

#[tokio::test]
async fn agent_never_sees_or_erases_engine_secrets() {
    let engine = engine().await;
    let secrets = json!({
        "proxy": "http://user:secret@127.0.0.1:9",
        "peers": [{"url": "http://127.0.0.1:9", "token": "peer-secret"}],
    });
    run_command(&engine, Access::Full, set(secrets, true))
        .await
        .unwrap();

    // The answer to a change holds the whole document: masked for the agent.
    let out = run_command(&engine, Access::Agent, set(json!({"paused": true}), true))
        .await
        .unwrap();
    assert_eq!(out["paused"], true);
    assert_eq!(out["proxy"], "***");
    assert_eq!(out["peers"][0]["token"], "***");

    // Replacing the document would erase the proxy and peers.
    let replace = run_command(&engine, Access::Agent, set(json!({"paused": false}), false)).await;
    assert_eq!(replace.unwrap_err().code, "forbidden");

    let full = run_command(&engine, Access::Full, set(json!({}), true))
        .await
        .unwrap();
    assert_eq!(full["proxy"], "http://user:secret@127.0.0.1:9");
    assert_eq!(full["peers"][0]["token"], "peer-secret");
    engine.shutdown().await;
}

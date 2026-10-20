//! TwitchMiner desktop launcher: the shared Slint interface plus desktop integration.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("tm-worker")
        .build()?;
    let opts = tm_ui::Options {
        // `--background`: open without taking the focus (used when starting with the session).
        background: std::env::args().any(|a| a == "--background"),
        ..Default::default()
    };
    tm_ui::run(runtime.handle().clone(), opts)?;
    // Give background tasks (engine shutdown) a moment to finish.
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    Ok(())
}

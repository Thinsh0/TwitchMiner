//! Native TwitchMiner interface (Slint), shared by the desktop launcher and, later,
//! Android.
//!
//! The interface only reads `Backend::state()` and only acts through
//! `Backend::execute(Command)`: the views (`ui/*.slint`) are a projection of the
//! engine state (`present`), and every callback becomes a command (`controller`).

// The demo fixture (`demo.rs`) is one large `json!` literal.
#![recursion_limit = "256"]

slint::include_modules!();

mod activity;
mod controller;
mod demo;
mod dev;
mod forms;
mod images;
mod platform;
mod prefs;
mod present;
mod session;
mod settings;

use std::path::PathBuf;

pub use platform::{AppHandle, NoPlatform, Platform, Status};
pub use session::Target;

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Data directory of the local engine (see `tm_core::Paths::resolve`).
    pub data_dir: Option<PathBuf>,
    /// Start connected to this daemon instead of a local engine.
    pub target: Option<Target>,
    /// Start in the background (session start): with a platform that keeps running
    /// (tray icon), no window is shown; otherwise it opens without taking the focus.
    pub background: bool,
}

/// Opens the main window and runs the event loop until the application quits.
/// `runtime` runs the engine and every command, off the UI thread.
pub fn run(
    runtime: tokio::runtime::Handle,
    opts: Options,
    platform: Box<dyn Platform>,
) -> anyhow::Result<()> {
    let keeps_running = platform.keeps_running();
    // Hidden in the tray when possible, else a window that does not take the focus.
    let hidden = opts.background && keeps_running;
    let inactive = opts.background && !keeps_running;
    // Development only: `TM_UI_SNAPSHOT=<file.bmp>` keeps the window outside the screen
    // (never covering what the developer is doing) and writes a capture of it every
    // second (`Window::take_snapshot`).
    let snapshot = std::env::var_os("TM_UI_SNAPSHOT").map(PathBuf::from);
    let offscreen = snapshot.is_some();
    let backend = slint::BackendSelector::new().with_winit_window_attributes_hook(move |attrs| {
        let attrs = attrs.with_active(!inactive && !offscreen);
        if offscreen {
            attrs.with_position(slint::winit_030::winit::dpi::PhysicalPosition::new(
                -20000, -20000,
            ))
        } else {
            attrs
        }
    });
    backend.select()?;
    let ui = MainWindow::new()?;
    let prefs = runtime.block_on(prefs::load(opts.data_dir.clone()));
    let env_lang = std::env::var("TM_LANG").ok();
    set_language(&ui, env_lang.as_deref().or(prefs.language.as_deref()));
    ui.global::<Theme>()
        .set_mode(theme_from_name(prefs.theme.as_deref()));
    ui.window().set_size(slint::LogicalSize::new(1180.0, 760.0));
    let demo = std::env::var_os("TM_UI_DEMO").map(|_| Target::Demo);
    let target = demo.or_else(|| opts.target.clone()).or_else(|| {
        prefs.remote.as_ref().map(|r| Target::Remote {
            url: r.url.clone(),
            token: r.token.clone(),
        })
    });
    let ctrl = controller::Controller::new(runtime.clone(), &ui, opts.data_dir, prefs);
    ctrl.bind(&ui);
    platform::install(platform);
    let has_autostart = platform::with(|p| p.autostart()).flatten();
    ui.global::<Bridge>()
        .set_has_autostart(has_autostart.is_some());
    ui.global::<Bridge>()
        .set_autostart(has_autostart.unwrap_or_default());
    ui.global::<Bridge>().set_keeps_running(keeps_running);
    platform::with(|p| {
        p.init(AppHandle {
            ui: ui.as_weak(),
            ctrl: ctrl.clone(),
        })
    });
    ctrl.open(target.unwrap_or(Target::Local), false);
    // Relative times ("in 12 min") move on even when the state does not.
    let tick = slint::Timer::default();
    tick.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_secs(30),
        {
            let weak = ui.as_weak();
            let ctrl = ctrl.clone();
            move || {
                if let Some(ui) = weak.upgrade() {
                    controller::render(&ui);
                }
                ctrl.tick();
            }
        },
    );
    let _snapshots = snapshot.map(|path| {
        dev::apply_env(&ui);
        dev::snapshot_every_second(&ui, path)
    });
    if keeps_running {
        // Closing the window only hides it; the tray menu quits.
        ui.window()
            .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
        if !hidden {
            ui.show()?;
        }
        slint::run_event_loop_until_quit()?;
    } else {
        ui.run()?;
    }
    runtime.block_on(ctrl.close());
    Ok(())
}

/// French unless English is asked (`TM_LANG=en` or the preference).
pub(crate) fn set_language(ui: &MainWindow, lang: Option<&str>) {
    let lang = match lang {
        Some(l) if l.to_lowercase().starts_with("en") => "en",
        _ => "fr",
    };
    if let Err(e) = slint::select_bundled_translation(lang) {
        tracing::warn!(error = %e, lang, "translation not available");
    }
    ui.global::<Bridge>()
        .set_language(if lang == "en" { 1 } else { 0 });
}

pub(crate) fn theme_from_name(name: Option<&str>) -> ThemeMode {
    match name {
        Some("light") => ThemeMode::Light,
        Some("oled") => ThemeMode::Oled,
        _ => ThemeMode::Dark,
    }
}

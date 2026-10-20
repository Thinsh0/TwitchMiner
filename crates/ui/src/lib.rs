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
mod prefs;
mod present;
mod session;
mod settings;

use std::path::PathBuf;

pub use session::Target;

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Data directory of the local engine (see `tm_core::Paths::resolve`).
    pub data_dir: Option<PathBuf>,
    /// Start connected to this daemon instead of a local engine.
    pub target: Option<Target>,
    /// Open the window without taking the focus (session start, launched from a
    /// script): a game in the foreground stays in the foreground.
    pub background: bool,
}

/// Opens the main window and runs the event loop until it closes. `runtime` runs the
/// engine and every command, off the UI thread.
pub fn run(runtime: tokio::runtime::Handle, opts: Options) -> anyhow::Result<()> {
    let background = opts.background;
    // Development only: `TM_UI_SNAPSHOT=<file.bmp>` keeps the window outside the screen
    // (never covering what the developer is doing) and writes a capture of it every
    // second (`Window::take_snapshot`).
    let snapshot = std::env::var_os("TM_UI_SNAPSHOT").map(PathBuf::from);
    let offscreen = snapshot.is_some();
    let backend = slint::BackendSelector::new().with_winit_window_attributes_hook(move |attrs| {
        let attrs = attrs.with_active(!background && !offscreen);
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
    ui.run()?;
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

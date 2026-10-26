//! Native TwitchMiner interface (Slint), shared by the desktop launcher and the
//! Android application (compact layout below 720 px, `Theme.compact`).
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
mod notifications;
mod platform;
mod prefs;
mod present;
mod session;
mod settings;
mod update;

use std::path::PathBuf;

pub use notifications::Notification;
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
    /// No local engine (phone): only a remote server can be used, asked for at the
    /// first start.
    pub remote_only: bool,
    /// Why the previous run stopped (a panic), shown once at start.
    pub last_crash: Option<String>,
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
    // Development only: `TM_UI_SNAPSHOT=<file.bmp>` keeps the window outside the screen
    // (never covering what the developer is doing) and writes a capture of it every
    // second (`Window::take_snapshot`).
    let snapshot = std::env::var_os("TM_UI_SNAPSHOT").map(PathBuf::from);
    #[cfg(not(target_os = "android"))]
    {
        let inactive = opts.background && !keeps_running;
        let offscreen = snapshot.is_some();
        let backend =
            slint::BackendSelector::new().with_winit_window_attributes_hook(move |attrs| {
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
    }
    let ui = MainWindow::new()?;
    let prefs = runtime.block_on(prefs::load(opts.data_dir.clone()));
    let env_lang = std::env::var("TM_LANG").ok();
    set_language(&ui, env_lang.as_deref().or(prefs.language.as_deref()));
    ui.global::<Theme>()
        .set_mode(theme_from_name(prefs.theme.as_deref()));
    // The window fills the screen on a phone.
    if !cfg!(target_os = "android") {
        ui.window().set_size(slint::LogicalSize::new(1180.0, 760.0));
    }
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
    ui.global::<Bridge>().set_remote_only(opts.remote_only);
    ui.global::<Bridge>()
        .set_on_phone(cfg!(target_os = "android"));
    match target.or((!opts.remote_only).then_some(Target::Local)) {
        Some(target) => ctrl.open(target, false),
        None => ctrl.show_setup(false),
    }
    update::cleanup();
    ui.global::<Bridge>()
        .set_app_version(update::CURRENT.into());
    ctrl.start_update_checks();
    if let Some(crash) = opts.last_crash {
        ctrl.report_crash(crash);
    }
    // Coming back to the window (Alt+Tab, click) refreshes what is not live; without
    // the focus, notable events become system notifications.
    #[cfg(not(target_os = "android"))]
    {
        use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
        let ctrl = ctrl.clone();
        ui.window().on_winit_window_event(move |_, event| {
            if let WindowEvent::Focused(focused) = event {
                ctrl.set_focused(*focused);
                if *focused {
                    ctrl.wake();
                }
            }
            EventResult::Propagate
        });
    }
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

/// HTTP client builder with the engine's TLS settings (bundled and system roots).
/// reqwest's default verifier asks the platform, which on Android needs a Java
/// component this application does not ship: the first HTTPS request would panic.
pub(crate) fn http_client() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .use_preconfigured_tls((*tm_core::twitch::tls::client_config()).clone())
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

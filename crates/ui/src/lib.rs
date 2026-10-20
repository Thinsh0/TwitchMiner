//! Native TwitchMiner interface (Slint), shared by the desktop launcher and, later,
//! Android.
//!
//! The interface only reads `Backend::state()` and only acts through
//! `Backend::execute(Command)`: the views (`ui/*.slint`) are a projection of the
//! engine state (`present`), and every callback becomes a command (`controller`).

slint::include_modules!();

mod controller;
mod dev;
mod present;
mod session;

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
    // second, rendered in memory.
    let snapshot = std::env::var_os("TM_UI_SNAPSHOT").map(PathBuf::from);
    let offscreen = snapshot.is_some();
    let mut backend =
        slint::BackendSelector::new().with_winit_window_attributes_hook(move |attrs| {
            let attrs = attrs.with_active(!background && !offscreen);
            if offscreen {
                attrs.with_position(slint::winit_030::winit::dpi::PhysicalPosition::new(
                    -20000, -20000,
                ))
            } else {
                attrs
            }
        });
    if offscreen {
        backend = backend.renderer_name("software".into());
    }
    backend.select()?;
    let ui = MainWindow::new()?;
    select_language();
    ui.window().set_size(slint::LogicalSize::new(1180.0, 760.0));
    let ctrl = controller::Controller::new(runtime.clone(), &ui, opts.data_dir);
    ctrl.bind(&ui);
    ctrl.open(opts.target.unwrap_or(Target::Local), false);
    // Relative times ("in 12 min") move on even when the state does not.
    let tick = slint::Timer::default();
    tick.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_secs(30),
        {
            let weak = ui.as_weak();
            move || {
                if let Some(ui) = weak.upgrade() {
                    controller::render(&ui);
                }
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

/// French by default (the primary language); `TM_LANG=en` switches to English.
fn select_language() {
    let lang = std::env::var("TM_LANG").unwrap_or_default().to_lowercase();
    let lang = if lang.starts_with("en") { "en" } else { "fr" };
    if let Err(e) = slint::select_bundled_translation(lang) {
        tracing::warn!(error = %e, lang, "translation not available");
    }
}

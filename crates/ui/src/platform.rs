//! What the host platform adds around the interface (tray icon, start with the
//! session, closing to the background), implemented by each launcher.

use std::cell::RefCell;

use slint::ComponentHandle;

use crate::MainWindow;
use crate::controller::Controller;
use crate::notifications::Notification;

/// Integration with the host platform, provided by the launcher. Every method runs
/// on the UI thread.
pub trait Platform {
    /// Called once, before the event loop runs.
    fn init(&mut self, _app: AppHandle) {}
    /// Called when the summary shown outside the window changes (tray tooltip, menu).
    fn status(&mut self, _status: &Status) {}
    /// Shows a system notification (only sent while the window is hidden).
    fn notify(&mut self, _notification: &Notification) {}
    /// Start with the session: `None` when not supported here.
    fn autostart(&self) -> Option<bool> {
        None
    }
    fn set_autostart(&self, _on: bool) -> Result<(), String> {
        Err("not supported".into())
    }
    /// Closing the window hides it; the application keeps running (tray icon).
    fn keeps_running(&self) -> bool {
        false
    }
    /// The application can replace its executable and start it again (`restart`).
    fn installs_updates(&self) -> bool {
        false
    }
    /// Starts `exe`, just updated; the application quits right after. The new process
    /// must wait for this one to be gone (single instance, local engine database).
    fn restart(&self, _exe: &std::path::Path) -> Result<(), String> {
        Err("not supported".into())
    }
}

/// No integration (plain window).
pub struct NoPlatform;

impl Platform for NoPlatform {}

/// Short summary for the tray icon (from `tm_core::view::summary`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    /// `fr` or `en`: the interface language.
    pub language: &'static str,
    pub paused: bool,
    /// Watching is allowed (not paused, inside the watching hours).
    pub watching_allowed: bool,
    /// Accounts needing the user (sign-in, errors).
    pub attention: usize,
    /// Channels watched right now (display names).
    pub watching: Vec<String>,
    /// Current drop: reward, game, progress in percent.
    pub drop: Option<(String, String, u8)>,
}

/// Drives the window and the engine from outside the views (tray menu, second
/// launch). Cheap to clone, usable from any thread.
#[derive(Clone)]
pub struct AppHandle {
    pub(crate) ui: slint::Weak<MainWindow>,
    pub(crate) ctrl: Controller,
}

impl AppHandle {
    /// Shows the window (restored and in front).
    pub fn show(&self) {
        let _ = self.ui.upgrade_in_event_loop(|ui| {
            let _ = ui.show();
            ui.window().set_minimized(false);
            use slint::winit_030::WinitWindowAccessor;
            ui.window().with_winit_window(|w| w.focus_window());
        });
        self.ctrl.wake();
    }

    pub fn hide(&self) {
        let _ = self.ui.upgrade_in_event_loop(|ui| {
            let _ = ui.hide();
        });
    }

    /// Global pause of the engine (`settings_set engine {paused}`).
    pub fn set_paused(&self, paused: bool) {
        self.ctrl.set_paused(paused);
    }

    /// Ends the event loop: the engine is then shut down cleanly.
    pub fn quit(&self) {
        let _ = slint::invoke_from_event_loop(|| {
            let _ = slint::quit_event_loop();
        });
    }
}

thread_local! {
    static PLATFORM: RefCell<Option<Box<dyn Platform>>> = const { RefCell::new(None) };
    static LAST_STATUS: RefCell<Option<Status>> = const { RefCell::new(None) };
}

pub(crate) fn install(p: Box<dyn Platform>) {
    PLATFORM.with(|cell| *cell.borrow_mut() = Some(p));
}

pub(crate) fn with<R>(f: impl FnOnce(&mut dyn Platform) -> R) -> Option<R> {
    PLATFORM.with(|cell| cell.borrow_mut().as_mut().map(|p| f(p.as_mut())))
}

/// Tells the platform about a new status, only when it changed.
pub(crate) fn publish(status: Status) {
    let changed = LAST_STATUS.with(|l| {
        let mut l = l.borrow_mut();
        if l.as_ref() == Some(&status) {
            false
        } else {
            *l = Some(status.clone());
            true
        }
    });
    if changed {
        with(|p| p.status(&status));
    }
}

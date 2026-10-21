//! TwitchMiner desktop launcher: the shared Slint interface plus desktop integration
//! (system tray, start with the session, one instance, closing to the tray).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod instance;
mod tray;

use tm_ui::{AppHandle, Notification, Platform, Status};

/// Argument used when starting with the session: start hidden in the tray.
const BACKGROUND: &str = "--background";

struct Desktop {
    tray: Option<tray::Tray>,
    listener: Option<std::net::TcpListener>,
}

impl Platform for Desktop {
    fn init(&mut self, app: AppHandle) {
        if let Some(tray) = &self.tray {
            tray.connect(app.clone());
        }
        if let Some(listener) = self.listener.take() {
            instance::serve(listener, move || app.show());
        }
    }

    fn status(&mut self, status: &Status) {
        if let Some(tray) = &mut self.tray {
            tray.update(status);
        }
    }

    fn notify(&mut self, n: &Notification) {
        let (title, body) = (n.title.clone(), n.body.clone());
        // Off the UI thread: the notification service may answer slowly.
        std::thread::spawn(move || {
            let shown = notify_rust::Notification::new()
                .appname("TwitchMiner")
                .summary(&title)
                .body(&body)
                .show();
            if let Err(e) = shown {
                eprintln!("notification not shown: {e}");
            }
        });
    }

    fn autostart(&self) -> Option<bool> {
        autostart().ok()?.is_enabled().ok()
    }

    fn set_autostart(&self, on: bool) -> Result<(), String> {
        let a = autostart().map_err(|e| e.to_string())?;
        if on { a.enable() } else { a.disable() }.map_err(|e| e.to_string())
    }

    fn keeps_running(&self) -> bool {
        // Without a tray icon, a hidden window could not be brought back.
        self.tray.is_some()
    }
}

fn autostart() -> anyhow::Result<auto_launch::AutoLaunch> {
    let exe = std::env::current_exe()?;
    Ok(auto_launch::AutoLaunchBuilder::new()
        .set_app_name("TwitchMiner")
        .set_app_path(&exe.to_string_lossy())
        .set_args(&[BACKGROUND])
        .set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser)
        .build()?)
}

fn main() -> anyhow::Result<()> {
    let background = std::env::args().any(|a| a == BACKGROUND);
    // Automated screenshots (development) run off-screen: no tray, no single instance.
    let dev_snapshot = std::env::var_os("TM_UI_SNAPSHOT").is_some();
    let listener = if dev_snapshot {
        None
    } else {
        match instance::acquire() {
            instance::Instance::Other => return Ok(()),
            instance::Instance::First(listener) => listener,
        }
    };
    let tray = if dev_snapshot {
        None
    } else {
        match tray::Tray::new() {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("no system tray icon: {e}");
                None
            }
        }
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("tm-worker")
        .build()?;
    let opts = tm_ui::Options {
        background,
        ..Default::default()
    };
    tm_ui::run(
        runtime.handle().clone(),
        opts,
        Box::new(Desktop { tray, listener }),
    )?;
    // Give background tasks (engine shutdown) a moment to finish.
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    Ok(())
}

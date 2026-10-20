//! Development helpers (automated screenshots). Not used in normal runs.

use std::path::PathBuf;
use std::time::Duration;

use slint::ComponentHandle;

use crate::MainWindow;

/// Writes a capture of the window to `path` (BMP) every second.
pub fn snapshot_every_second(ui: &MainWindow, path: PathBuf) -> slint::Timer {
    let timer = slint::Timer::default();
    let weak = ui.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(1),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            match ui.window().take_snapshot() {
                Ok(buf) => {
                    if let Err(e) = write_bmp(&path, buf.width(), buf.height(), buf.as_bytes()) {
                        tracing::warn!(error = %e, "snapshot not written");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "snapshot failed"),
            }
        },
    );
    timer
}

/// 32-bit top-down BMP from RGBA pixels.
fn write_bmp(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    let size = 54 + rgba.len() as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for px in rgba.chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(tmp, path)
}

/// `TM_UI_SCREEN` (home, drops, channels, activity, settings), `TM_UI_THEME`
/// (dark, light, oled) and `TM_UI_SIZE` (`1180x1000`): start on a given view, for
/// screenshots.
pub fn apply_env(ui: &MainWindow) {
    if let Ok(size) = std::env::var("TM_UI_SIZE")
        && let Some((w, h)) = size.split_once('x')
        && let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>())
    {
        ui.window().set_size(slint::LogicalSize::new(w, h));
    }
    use crate::{Bridge, Screen, Theme};
    if let Ok(s) = std::env::var("TM_UI_SCREEN") {
        let screen = match s.as_str() {
            "drops" => Screen::Drops,
            "channels" => Screen::Channels,
            "activity" => Screen::Activity,
            "settings" => Screen::Settings,
            _ => Screen::Home,
        };
        ui.global::<Bridge>().set_screen(screen);
    }
    if let Ok(t) = std::env::var("TM_UI_THEME") {
        ui.global::<Theme>()
            .set_mode(crate::theme_from_name(Some(&t)));
    }
}

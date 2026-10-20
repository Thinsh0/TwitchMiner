//! System tray icon: summary as tooltip, menu (open, pause / resume, quit), left
//! click opens the window.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tm_ui::{AppHandle, Status};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Texts of the tray (it is outside the Slint views and their translations).
struct Texts {
    open: &'static str,
    pause: &'static str,
    resume: &'static str,
    quit: &'static str,
    paused: &'static str,
    attention: &'static str,
    outside_hours: &'static str,
    idle: &'static str,
    watching: &'static str,
    drop: &'static str,
}

fn texts(lang: &str) -> Texts {
    if lang == "en" {
        Texts {
            open: "Open TwitchMiner",
            pause: "Pause",
            resume: "Resume",
            quit: "Quit",
            paused: "Paused",
            attention: "Action needed: open TwitchMiner",
            outside_hours: "Outside watching hours",
            idle: "Nothing to watch right now",
            watching: "Watching",
            drop: "Drop",
        }
    } else {
        Texts {
            open: "Ouvrir TwitchMiner",
            pause: "Mettre en pause",
            resume: "Reprendre",
            quit: "Quitter",
            paused: "En pause",
            attention: "Action requise : ouvre TwitchMiner",
            outside_hours: "Hors des plages horaires",
            idle: "Rien à regarder pour l'instant",
            watching: "Regarde",
            drop: "Drop",
        }
    }
}

pub struct Tray {
    icon: TrayIcon,
    open: MenuItem,
    pause: MenuItem,
    quit: MenuItem,
    paused: Arc<AtomicBool>,
}

impl Tray {
    pub fn new() -> anyhow::Result<Self> {
        let t = texts("fr");
        let open = MenuItem::new(t.open, true, None);
        let pause = MenuItem::new(t.pause, true, None);
        let quit = MenuItem::new(t.quit, true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &open,
            &PredefinedMenuItem::separator(),
            &pause,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("TwitchMiner")
            .with_icon(icon()?)
            .build()?;
        Ok(Self {
            icon,
            open,
            pause,
            quit,
            paused: Arc::default(),
        })
    }

    /// Connects the menu and the click to the application.
    pub fn connect(&self, app: AppHandle) {
        let (open, pause, quit) = (
            self.open.id().clone(),
            self.pause.id().clone(),
            self.quit.id().clone(),
        );
        let paused = self.paused.clone();
        let menu_app = app.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if e.id == open {
                menu_app.show();
            } else if e.id == pause {
                menu_app.set_paused(!paused.load(Ordering::Relaxed));
            } else if e.id == quit {
                menu_app.quit();
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                app.show();
            }
        }));
    }

    pub fn update(&mut self, s: &Status) {
        let t = texts(s.language);
        self.paused.store(s.paused, Ordering::Relaxed);
        self.open.set_text(t.open);
        self.pause
            .set_text(if s.paused { t.resume } else { t.pause });
        self.quit.set_text(t.quit);
        let _ = self.icon.set_tooltip(Some(tooltip(s, &t)));
    }
}

/// `TwitchMiner · Regarde RustGod, ZeratoR` + the current drop (Windows keeps 127
/// characters).
fn tooltip(s: &Status, t: &Texts) -> String {
    let line = if s.attention > 0 {
        t.attention.to_string()
    } else if s.paused {
        t.paused.to_string()
    } else if !s.watching_allowed {
        t.outside_hours.to_string()
    } else if s.watching.is_empty() {
        t.idle.to_string()
    } else {
        format!("{} {}", t.watching, s.watching.join(", "))
    };
    let mut text = format!("TwitchMiner · {line}");
    if let Some((reward, game, pct)) = &s.drop
        && !s.paused
    {
        text.push_str(&format!("\n{} {reward} ({game}) {pct} %", t.drop));
    }
    text.chars().take(127).collect()
}

fn icon() -> anyhow::Result<tray_icon::Icon> {
    let img = image::load_from_memory(include_bytes!("../assets/icon-64.png"))?.to_rgba8();
    let (w, h) = img.dimensions();
    Ok(tray_icon::Icon::from_rgba(img.into_raw(), w, h)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_says_what_matters_first() {
        let mut s = Status {
            language: "fr",
            watching_allowed: true,
            watching: vec!["RustGod".into(), "ZeratoR".into()],
            drop: Some(("Tempered AK-47".into(), "Rust".into(), 65)),
            ..Default::default()
        };
        assert_eq!(
            tooltip(&s, &texts("fr")),
            "TwitchMiner · Regarde RustGod, ZeratoR\nDrop Tempered AK-47 (Rust) 65 %"
        );
        s.paused = true;
        assert_eq!(tooltip(&s, &texts("fr")), "TwitchMiner · En pause");
        s.attention = 1;
        assert!(tooltip(&s, &texts("en")).contains("Action needed"));
    }
}

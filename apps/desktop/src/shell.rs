//! Windows shell integration: a Start menu shortcut and an entry in the installed apps
//! (Settings › Apps), refreshed at start so they follow the executable if it moves.
//! The entry's uninstall command (`--uninstall`) removes both, and the autostart entry.
//! Elsewhere these are no-ops.

/// Argument of the uninstall command registered with Windows.
pub const UNINSTALL: &str = "--uninstall";

#[cfg(windows)]
pub use imp::{register, uninstall};

#[cfg(not(windows))]
pub fn register() {}

#[cfg(not(windows))]
pub fn uninstall() {}

#[cfg(windows)]
mod imp {
    use std::path::{Path, PathBuf};

    use windows_registry::CURRENT_USER;
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, MessageBoxW,
    };

    const APP: &str = "TwitchMiner";
    const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\TwitchMiner";

    fn shortcut() -> Option<PathBuf> {
        let appdata = std::env::var_os("APPDATA")?;
        Some(
            PathBuf::from(appdata)
                .join(r"Microsoft\Windows\Start Menu\Programs")
                .join(format!("{APP}.lnk")),
        )
    }

    /// Creates or updates the shortcut and the installed apps entry. Errors are only
    /// logged: the application works without them.
    pub fn register() {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let icon = format!("{},0", exe.display());
        let up_to_date = CURRENT_USER
            .open(UNINSTALL_KEY)
            .and_then(|k| k.get_string("DisplayIcon"))
            .is_ok_and(|v| v == icon)
            && shortcut().is_some_and(|p| p.exists());
        if up_to_date {
            return;
        }
        if let Err(e) = write_shortcut(&exe) {
            eprintln!("start menu shortcut not created: {e}");
        }
        if let Err(e) = write_entry(&exe, &icon) {
            eprintln!("installed apps entry not created: {e}");
        }
    }

    fn write_shortcut(exe: &Path) -> Result<(), String> {
        let path = shortcut().ok_or("APPDATA not set")?;
        let mut link = mslnk::ShellLink::new(exe).map_err(|e| e.to_string())?;
        link.set_working_dir(exe.parent().map(|d| d.display().to_string()));
        link.set_name(Some(APP.to_string()));
        link.create_lnk(&path).map_err(|e| e.to_string())
    }

    fn write_entry(exe: &Path, icon: &str) -> windows_registry::Result<()> {
        let key = CURRENT_USER.create(UNINSTALL_KEY)?;
        key.set_string("DisplayName", APP)?;
        key.set_string("DisplayVersion", env!("CARGO_PKG_VERSION"))?;
        key.set_string("Publisher", APP)?;
        key.set_string("DisplayIcon", icon)?;
        if let Some(dir) = exe.parent() {
            key.set_string("InstallLocation", dir.display().to_string())?;
        }
        key.set_string(
            "UninstallString",
            format!("\"{}\" {}", exe.display(), super::UNINSTALL),
        )?;
        key.set_u32("NoModify", 1)?;
        key.set_u32("NoRepair", 1)?;
        if let Ok(meta) = std::fs::metadata(exe) {
            key.set_u32("EstimatedSize", (meta.len() / 1024) as u32)?;
        }
        Ok(())
    }

    /// Asks, then removes the shortcut, the installed apps entry and the autostart entry.
    /// Data and the executable stay: the folder is the user's to delete.
    pub fn uninstall() {
        let fr = is_french();
        let question = if fr {
            "Retirer TwitchMiner du menu Démarrer et de la liste des applications ?\n\n\
             Tes données (comptes, réglages, historique) sont conservées."
        } else {
            "Remove TwitchMiner from the Start menu and the apps list?\n\n\
             Your data (accounts, settings, history) is kept."
        };
        if message(question, MB_YESNO | MB_ICONQUESTION) != IDYES {
            return;
        }
        if let Some(p) = shortcut() {
            let _ = std::fs::remove_file(p);
        }
        let _ = CURRENT_USER.remove_tree(UNINSTALL_KEY);
        if let Ok(a) = crate::autostart() {
            let _ = a.disable();
        }
        let dir = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.display().to_string()))
            .unwrap_or_default();
        let done = if fr {
            format!(
                "TwitchMiner a été retiré.\n\nQuitte l'application si elle tourne encore, \
                 puis supprime ce dossier si tu veux : {dir}"
            )
        } else {
            format!(
                "TwitchMiner was removed.\n\nQuit the application if it is still running, \
                 then delete this folder if you want: {dir}"
            )
        };
        message(&done, MB_OK | MB_ICONINFORMATION);
    }

    fn is_french() -> bool {
        // SAFETY: no arguments, no side effects.
        let lang = unsafe { GetUserDefaultUILanguage() };
        lang & 0x3ff == 0x0c // LANG_FRENCH
    }

    fn message(text: &str, style: u32) -> i32 {
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (text, title) = (wide(text), wide(APP));
        // SAFETY: both strings are NUL-terminated UTF-16 and outlive the call.
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), style) }
    }
}

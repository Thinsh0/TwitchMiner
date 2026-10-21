//! The data directory: the one place where everything is stored.
//!
//! ```text
//! <data>/
//!   twitchminer.db   accounts, settings, history, statistics, module data
//!   twitchminer.lock held by the running engine (one engine per data directory)
//!   logs/            rolling log files
//! ```
//!
//! Resolution order: explicit path > `TM_DATA_DIR` > `./data` next to the executable
//! when a `portable` file exists there > the platform data directory.

use std::path::{Path, PathBuf};

/// The database holds Twitch and API tokens: on Unix, only the owner may enter the
/// data directory (`0700`), which covers every file in it. Best effort: a directory
/// owned by someone else is left as it is.
fn restrict(root: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)) {
            tracing::warn!(dir = %root.display(), error = %e, "could not restrict the data directory");
        }
    }
    #[cfg(not(unix))]
    let _ = root;
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn resolve(explicit: Option<PathBuf>) -> std::io::Result<Self> {
        let root = explicit
            .or_else(|| std::env::var_os("TM_DATA_DIR").map(PathBuf::from))
            .or_else(portable_dir)
            .or_else(|| {
                directories::ProjectDirs::from("dev", "Thinsh0", "TwitchMiner")
                    .map(|d| d.data_dir().to_path_buf())
            })
            .unwrap_or_else(|| PathBuf::from("data"));
        Self::at(root)
    }

    pub fn at(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(root.join("logs"))?;
        restrict(&root);
        Ok(Self { root })
    }

    pub fn db_file(&self) -> PathBuf {
        self.root.join("twitchminer.db")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Takes the exclusive lock of this data directory. Fails with
    /// [`LockError::Busy`] if another engine (daemon, desktop app…) holds it.
    /// The lock is released when the returned guard is dropped or the process dies.
    pub fn lock(&self) -> Result<DataLock, LockError> {
        use std::io::{Read, Seek, Write};
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("twitchminer.lock"))
            .map_err(LockError::Io)?;
        match file.try_lock() {
            Ok(()) => {
                let _ = file.set_len(0);
                let _ = file.rewind();
                let _ = write!(file, "{}", std::process::id());
                Ok(DataLock { _file: file })
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                let mut holder = String::new();
                let _ = file.read_to_string(&mut holder);
                Err(LockError::Busy {
                    pid: holder.trim().to_string(),
                })
            }
            Err(std::fs::TryLockError::Error(e)) => Err(LockError::Io(e)),
        }
    }
}

/// Exclusive lock on a data directory; released on drop.
#[derive(Debug)]
pub struct DataLock {
    _file: std::fs::File,
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("another TwitchMiner instance (pid {pid}) is already using this data directory")]
    Busy { pid: String },
    #[error("cannot lock the data directory: {0}")]
    Io(std::io::Error),
}

fn portable_dir() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    exe_dir
        .join("portable")
        .exists()
        .then(|| exe_dir.join("data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_engine_per_data_dir() {
        let paths = Paths::at(std::env::temp_dir().join(format!("tm-lock-{}", std::process::id())))
            .unwrap();
        let first = paths.lock().unwrap();
        assert!(matches!(paths.lock(), Err(LockError::Busy { .. })));
        drop(first);
        assert!(paths.lock().is_ok(), "released on drop");
        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owner_enters_the_data_dir() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("tm-perm-{}", std::process::id()));
        let paths = Paths::at(&dir).unwrap();
        let mode = std::fs::metadata(&paths.root).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&paths.root);
    }
}

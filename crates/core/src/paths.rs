//! The data directory: the one place where everything is stored.
//!
//! ```text
//! <data>/
//!   twitchminer.db   accounts, settings, history, statistics, module data
//!   logs/            rolling log files
//! ```
//!
//! Resolution order: explicit path > `TM_DATA_DIR` > `./data` next to the executable
//! when a `portable` file exists there > the platform data directory.

use std::path::{Path, PathBuf};

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
        Ok(Self { root })
    }

    pub fn db_file(&self) -> PathBuf {
        self.root.join("twitchminer.db")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
}

fn portable_dir() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    exe_dir
        .join("portable")
        .exists()
        .then(|| exe_dir.join("data"))
}

//! Interface preferences (theme, language, last remote server), stored in the data
//! directory's database through `tm_app::{read,write}_ui_prefs`. View settings only:
//! nothing here is engine state.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// `dark` | `light` | `oled`
    pub theme: Option<String>,
    /// `fr` | `en`
    pub language: Option<String>,
    /// Engine to reconnect to at start; `None` = local engine.
    pub remote: Option<RemotePrefs>,
    /// Address the local engine serves its API on (`127.0.0.1:7878`); `None` = no API.
    pub api: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemotePrefs {
    pub url: String,
    pub token: String,
}

pub async fn load(data_dir: Option<PathBuf>) -> Prefs {
    match tm_app::read_ui_prefs(data_dir).await {
        Ok(v) if !v.is_null() => serde_json::from_value(v).unwrap_or_default(),
        Ok(_) => Prefs::default(),
        Err(e) => {
            tracing::warn!(error = %e, "cannot read interface preferences");
            Prefs::default()
        }
    }
}

pub async fn save(data_dir: Option<PathBuf>, prefs: Prefs) {
    let value = match serde_json::to_value(&prefs) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "cannot encode interface preferences");
            return;
        }
    };
    if let Err(e) = tm_app::write_ui_prefs(data_dir, value).await {
        tracing::warn!(error = %e, "cannot save interface preferences");
    }
}

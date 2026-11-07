//! Persisted desktop preferences.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::{DEFAULT_BASE_URL, normalize_base_url};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server_url: String,
    #[serde(default)]
    pub client_side_decorations: bool,
    #[serde(default)]
    pub remember_login: bool,
    #[serde(default)]
    pub remembered_email: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: DEFAULT_BASE_URL.to_owned(),
            client_side_decorations: false,
            remember_login: false,
            remembered_email: None,
        }
    }
}

impl Config {
    /// Load the config, falling back to defaults if it is missing or invalid.
    pub fn load() -> Self {
        let mut config = config_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|contents| serde_json::from_str::<Self>(&contents).ok())
            .unwrap_or_default();

        match normalize_base_url(&config.server_url) {
            Ok(url) => config.server_url = url,
            Err(_) => config.server_url = DEFAULT_BASE_URL.to_owned(),
        }

        config
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path().ok_or_else(|| "could not locate a config directory".to_owned())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("could not create {}: {err}", parent.display()))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|err| format!("could not serialise settings: {err}"))?;
        fs::write(&path, json).map_err(|err| format!("could not write {}: {err}", path.display()))
    }
}

/// Location of the config file, if a platform config directory is available.
pub fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("au", "CareLibre", "CareLibre Desktop")
        .map(|dirs| dirs.config_dir().join("config.json"))
}

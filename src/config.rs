//! Persisted desktop preferences.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::{DEFAULT_BASE_URL, normalize_base_url};
use crate::theme::ThemeMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server_url: String,
    #[serde(default)]
    pub theme: ThemeMode,
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
            theme: ThemeMode::System,
            // New installs get CareLibre's own title bar: the Wayland fallback
            // decorations some compositors draw are inconsistent and look foreign.
            // Existing configs that predate this field keep native decorations,
            // because the field-level serde default below is `false`.
            client_side_decorations: true,
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

#[cfg(test)]
mod tests {
    use super::Config;
    use crate::theme::ThemeMode;

    #[test]
    fn new_installs_get_client_side_chrome() {
        assert!(Config::default().client_side_decorations);
    }

    #[test]
    fn new_installs_follow_the_system_theme() {
        assert_eq!(Config::default().theme, ThemeMode::System);
    }

    #[test]
    fn existing_configs_without_the_theme_field_follow_the_system() {
        let config: Config =
            serde_json::from_str(r#"{"server_url":"http://127.0.0.1:8484"}"#).unwrap();
        assert_eq!(config.theme, ThemeMode::System);
    }

    #[test]
    fn saved_theme_preference_wins() {
        let config: Config =
            serde_json::from_str(r#"{"server_url":"http://127.0.0.1:8484","theme":"light"}"#)
                .unwrap();
        assert_eq!(config.theme, ThemeMode::Light);
    }

    #[test]
    fn existing_configs_without_the_field_keep_native_decorations() {
        let config: Config =
            serde_json::from_str(r#"{"server_url":"http://127.0.0.1:8484"}"#).unwrap();
        assert!(!config.client_side_decorations);
    }

    #[test]
    fn saved_preference_wins() {
        let config: Config = serde_json::from_str(
            r#"{"server_url":"http://127.0.0.1:8484","client_side_decorations":false}"#,
        )
        .unwrap();
        assert!(!config.client_side_decorations);
    }
}

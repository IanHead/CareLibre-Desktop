//! Persisted desktop preferences.

use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::{DEFAULT_BASE_URL, normalize_base_url};
use crate::theme::ThemeMode;

/// Lower bound for the relationship constellation's step control.
pub const CONSTELLATION_MIN_STEPS: u8 = 1;
/// Upper bound keeps dense tenant graphs readable.
pub const CONSTELLATION_MAX_STEPS: u8 = 4;
/// New records start at two steps, matching the roadmap.
pub const CONSTELLATION_DEFAULT_STEPS: u8 = 2;

/// How one user last left the relationship constellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RelationshipViewPreference {
    pub collapsed: bool,
    pub steps: u8,
}

impl Default for RelationshipViewPreference {
    fn default() -> Self {
        Self {
            collapsed: true,
            steps: CONSTELLATION_DEFAULT_STEPS,
        }
    }
}

impl RelationshipViewPreference {
    /// Keep preferences loaded from older configs or forwarded by a server
    /// inside the supported range.
    pub fn normalised(mut self) -> Self {
        self.steps = self
            .steps
            .clamp(CONSTELLATION_MIN_STEPS, CONSTELLATION_MAX_STEPS);
        self
    }
}

/// Preferences are scoped to the signed-in account, not just the machine, so
/// two people sharing a desktop do not inherit each other's view settings.
pub fn relationship_view_key(server_url: &str, organisation_id: &str, user_id: &str) -> String {
    format!("{server_url}|{organisation_id}|{user_id}")
}

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
    #[serde(default)]
    pub relationship_views: BTreeMap<String, RelationshipViewPreference>,
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
            relationship_views: BTreeMap::new(),
        }
    }
}

impl Config {
    /// The constellation preference for an account, defaulting to collapsed
    /// with two steps on first use.
    pub fn relationship_view(&self, key: &str) -> RelationshipViewPreference {
        self.relationship_views
            .get(key)
            .copied()
            .unwrap_or_default()
            .normalised()
    }

    pub fn set_relationship_view(&mut self, key: String, preference: RelationshipViewPreference) {
        self.relationship_views.insert(key, preference.normalised());
    }

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
    use super::{
        CONSTELLATION_DEFAULT_STEPS, CONSTELLATION_MAX_STEPS, CONSTELLATION_MIN_STEPS, Config,
        RelationshipViewPreference, relationship_view_key,
    };
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

    #[test]
    fn constellation_defaults_to_collapsed_with_two_steps() {
        let preference = RelationshipViewPreference::default();
        assert!(preference.collapsed);
        assert_eq!(preference.steps, CONSTELLATION_DEFAULT_STEPS);
    }

    #[test]
    fn old_configs_have_no_constellation_preferences() {
        let config: Config =
            serde_json::from_str(r#"{"server_url":"http://127.0.0.1:8484"}"#).unwrap();
        assert!(config.relationship_views.is_empty());
        assert_eq!(
            config.relationship_view("server|org|user"),
            RelationshipViewPreference::default()
        );
    }

    #[test]
    fn constellation_preferences_are_scoped_per_account() {
        let mut config = Config::default();
        let alice = relationship_view_key("http://server", "org-a", "user-a");
        let bob = relationship_view_key("http://server", "org-a", "user-b");

        config.set_relationship_view(
            alice.clone(),
            RelationshipViewPreference {
                collapsed: false,
                steps: 4,
            },
        );

        assert_eq!(config.relationship_view(&alice).steps, 4);
        assert!(!config.relationship_view(&alice).collapsed);
        assert_eq!(
            config.relationship_view(&bob),
            RelationshipViewPreference::default()
        );
    }

    #[test]
    fn out_of_range_steps_are_clamped() {
        let preference = RelationshipViewPreference {
            collapsed: true,
            steps: 99,
        };
        assert_eq!(preference.normalised().steps, CONSTELLATION_MAX_STEPS);
        let preference = RelationshipViewPreference {
            collapsed: false,
            steps: 0,
        };
        assert_eq!(preference.normalised().steps, CONSTELLATION_MIN_STEPS);
    }

    #[test]
    fn constellation_preferences_round_trip() {
        let mut config = Config::default();
        let key = relationship_view_key("http://server", "org", "user");
        config.set_relationship_view(
            key.clone(),
            RelationshipViewPreference {
                collapsed: false,
                steps: 3,
            },
        );

        let json = serde_json::to_string(&config).unwrap();
        let reloaded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(reloaded.relationship_view(&key).steps, 3);
        assert!(!reloaded.relationship_view(&key).collapsed);
    }
}

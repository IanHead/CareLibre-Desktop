//! Persisted desktop preferences.

use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::{DEFAULT_BASE_URL, normalize_base_url};
use crate::theme::ThemeMode;

/// Lower bound for the relationship constellation's step control.
pub const CONSTELLATION_MIN_STEPS: u8 = 1;
/// Upper bound keeps dense tenant graphs readable.
pub const CONSTELLATION_MAX_STEPS: u8 = 4;
/// New records start one step deep, showing only direct connections.
pub const CONSTELLATION_DEFAULT_STEPS: u8 = 1;

/// How one user last left the relationship constellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RelationshipViewPreference {
    pub collapsed: bool,
    pub steps: u8,
    /// Show highly connected parties without expanding their neighbours.
    pub skip_hubs: bool,
}

impl Default for RelationshipViewPreference {
    fn default() -> Self {
        Self {
            collapsed: true,
            steps: CONSTELLATION_DEFAULT_STEPS,
            skip_hubs: true,
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

/// Lower bound for the relationship explorer's level control.
pub const EXPLORER_MIN_LEVELS: u8 = 1;
/// The explorer is a large, zoomable canvas, so it can show more depth than a
/// record's constellation before it becomes unreadable.
pub const EXPLORER_MAX_LEVELS: u8 = 6;
/// The explorer opens with the focus and its immediate neighbours.
pub const EXPLORER_DEFAULT_LEVELS: u8 = 2;

/// How one user last left the relationship explorer. The focus party is a
/// navigation choice, so it is not persisted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExplorerPreference {
    pub levels: u8,
    pub show_ended: bool,
    pub show_contacts: bool,
    pub show_organisations: bool,
    /// Show highly connected parties without expanding their neighbours.
    pub skip_hubs: bool,
}

impl Default for ExplorerPreference {
    fn default() -> Self {
        Self {
            levels: EXPLORER_DEFAULT_LEVELS,
            show_ended: false,
            show_contacts: true,
            show_organisations: true,
            skip_hubs: true,
        }
    }
}

impl ExplorerPreference {
    pub fn normalised(mut self) -> Self {
        self.levels = self.levels.clamp(EXPLORER_MIN_LEVELS, EXPLORER_MAX_LEVELS);
        // Hiding both party kinds would leave only the focus node, which is
        // never what the user means: keep contacts as the floor.
        if !self.show_contacts && !self.show_organisations {
            self.show_contacts = true;
        }
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
    /// Whether the docked main menu is collapsed to its icon rail.
    #[serde(default)]
    pub menu_collapsed: bool,
    #[serde(default)]
    pub remember_login: bool,
    #[serde(default)]
    pub remembered_email: Option<String>,
    #[serde(default)]
    pub relationship_views: BTreeMap<String, RelationshipViewPreference>,
    /// Explorer level and filter settings, keyed the same way as the
    /// constellation preferences.
    #[serde(default)]
    pub explorer_views: BTreeMap<String, ExplorerPreference>,
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
            menu_collapsed: false,
            remember_login: false,
            remembered_email: None,
            relationship_views: BTreeMap::new(),
            explorer_views: BTreeMap::new(),
        }
    }
}

impl Config {
    /// The constellation preference for an account, defaulting to collapsed
    /// with one step on first use.
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

    /// The relationship explorer settings for an account, defaulting to two
    /// levels with active contacts and organisations.
    pub fn explorer_view(&self, key: &str) -> ExplorerPreference {
        self.explorer_views
            .get(key)
            .copied()
            .unwrap_or_default()
            .normalised()
    }

    pub fn set_explorer_view(&mut self, key: String, preference: ExplorerPreference) {
        self.explorer_views.insert(key, preference.normalised());
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
        EXPLORER_DEFAULT_LEVELS, EXPLORER_MAX_LEVELS, EXPLORER_MIN_LEVELS, ExplorerPreference,
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
    fn the_menu_starts_expanded() {
        assert!(!Config::default().menu_collapsed);
    }

    #[test]
    fn old_configs_keep_the_menu_expanded() {
        let config: Config =
            serde_json::from_str(r#"{"server_url":"http://127.0.0.1:8484"}"#).unwrap();
        assert!(!config.menu_collapsed);
    }

    #[test]
    fn a_collapsed_menu_preference_is_remembered() {
        let config: Config =
            serde_json::from_str(r#"{"server_url":"http://127.0.0.1:8484","menu_collapsed":true}"#)
                .unwrap();
        assert!(config.menu_collapsed);
    }

    #[test]
    fn constellation_defaults_to_collapsed_with_one_step() {
        let preference = RelationshipViewPreference::default();
        assert!(preference.collapsed);
        assert_eq!(preference.steps, CONSTELLATION_DEFAULT_STEPS);
        assert_eq!(preference.steps, 1);
        assert!(preference.skip_hubs);
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
                skip_hubs: true,
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
            skip_hubs: true,
        };
        assert_eq!(preference.normalised().steps, CONSTELLATION_MAX_STEPS);
        let preference = RelationshipViewPreference {
            collapsed: false,
            steps: 0,
            skip_hubs: true,
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
                skip_hubs: true,
            },
        );

        let json = serde_json::to_string(&config).unwrap();
        let reloaded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(reloaded.relationship_view(&key).steps, 3);
        assert!(!reloaded.relationship_view(&key).collapsed);
    }

    #[test]
    fn explorer_defaults_to_two_levels() {
        let preference = ExplorerPreference::default();
        assert_eq!(preference.levels, EXPLORER_DEFAULT_LEVELS);
        assert!(!preference.show_ended);
        assert!(preference.show_contacts);
        assert!(preference.show_organisations);
        assert!(preference.skip_hubs);
    }

    #[test]
    fn old_configs_have_no_explorer_preferences() {
        let config: Config =
            serde_json::from_str(r#"{"server_url":"http://127.0.0.1:8484"}"#).unwrap();
        assert!(config.explorer_views.is_empty());
        assert_eq!(
            config.explorer_view("server|org|user"),
            ExplorerPreference::default()
        );
    }

    #[test]
    fn explorer_preferences_are_scoped_per_account() {
        let mut config = Config::default();
        let alice = relationship_view_key("http://server", "org-a", "user-a");
        let bob = relationship_view_key("http://server", "org-a", "user-b");
        config.set_explorer_view(
            alice.clone(),
            ExplorerPreference {
                levels: 4,
                show_ended: true,
                show_contacts: false,
                show_organisations: true,
                skip_hubs: true,
            },
        );

        assert_eq!(config.explorer_view(&alice).levels, 4);
        assert!(config.explorer_view(&alice).show_ended);
        assert_eq!(config.explorer_view(&bob), ExplorerPreference::default());
    }

    #[test]
    fn explorer_levels_are_clamped_and_one_kind_stays_visible() {
        let preference = ExplorerPreference {
            levels: 99,
            show_ended: false,
            show_contacts: false,
            show_organisations: false,
            skip_hubs: true,
        }
        .normalised();
        assert_eq!(preference.levels, EXPLORER_MAX_LEVELS);
        assert!(preference.show_contacts);

        let preference = ExplorerPreference {
            levels: 0,
            ..ExplorerPreference::default()
        }
        .normalised();
        assert_eq!(preference.levels, EXPLORER_MIN_LEVELS);
    }

    #[test]
    fn preferences_saved_before_hub_suppression_default_to_skipping_hubs() {
        let config: Config = serde_json::from_str(
            r#"{
                "relationship_views": {"key": {"collapsed": false, "steps": 3}},
                "explorer_views": {"key": {"levels": 4, "show_ended": true}}
            }"#,
        )
        .unwrap();
        assert!(config.relationship_view("key").skip_hubs);
        assert!(config.explorer_view("key").skip_hubs);
        assert_eq!(config.relationship_view("key").steps, 3);
        assert_eq!(config.explorer_view("key").levels, 4);
    }

    #[test]
    fn explorer_preferences_round_trip() {
        let mut config = Config::default();
        let key = relationship_view_key("http://server", "org", "user");
        config.set_explorer_view(
            key.clone(),
            ExplorerPreference {
                levels: 3,
                show_ended: true,
                show_contacts: true,
                show_organisations: false,
                skip_hubs: true,
            },
        );

        let json = serde_json::to_string(&config).unwrap();
        let reloaded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(reloaded.explorer_view(&key).levels, 3);
        assert!(reloaded.explorer_view(&key).show_ended);
        assert!(!reloaded.explorer_view(&key).show_organisations);
    }
}

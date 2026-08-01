use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuPreference {
    #[default]
    Auto,
    Integrated,
    Discrete,
}

impl fmt::Display for GpuPreference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuPreference::Auto => write!(f, "auto"),
            GpuPreference::Integrated => write!(f, "integrated"),
            GpuPreference::Discrete => write!(f, "discrete"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl fmt::Display for ThemePreference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemePreference::System => write!(f, "system"),
            ThemePreference::Light => write!(f, "light"),
            ThemePreference::Dark => write!(f, "dark"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub zoom_level: u16,
    pub dnd: bool,
    #[serde(default)]
    pub gpu_preference: GpuPreference,
    #[serde(default)]
    pub theme_preference: ThemePreference,
    #[serde(default = "default_auto_check_updates")]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub last_update_check_unix: i64,
}

fn default_auto_check_updates() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zoom_level: 10,
            dnd: false,
            gpu_preference: GpuPreference::Auto,
            theme_preference: ThemePreference::System,
            auto_check_updates: true,
            last_update_check_unix: 0,
        }
    }
}

impl Settings {
    pub fn load(data_dir: &std::path::Path) -> Self {
        let path = data_dir.join("settings.json");
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, data_dir: &std::path::Path) {
        let path = data_dir.join("settings.json");
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_missing() {
        let dir = std::env::temp_dir().join("slackinux_settings_missing");
        let _ = std::fs::remove_dir_all(&dir);
        let settings = Settings::load(&dir);
        assert_eq!(settings.zoom_level, 10);
        assert!(!settings.dnd);
        assert_eq!(settings.gpu_preference, GpuPreference::Auto);
        assert_eq!(settings.theme_preference, ThemePreference::System);
        assert!(settings.auto_check_updates);
        assert_eq!(settings.last_update_check_unix, 0);
    }

    #[test]
    fn round_trip_save_load() {
        let dir = std::env::temp_dir().join("slackinux_settings_roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let settings = Settings {
            zoom_level: 15,
            dnd: true,
            gpu_preference: GpuPreference::Discrete,
            theme_preference: ThemePreference::Dark,
            auto_check_updates: false,
            last_update_check_unix: 1_700_000_000,
        };
        settings.save(&dir);

        let loaded = Settings::load(&dir);
        assert_eq!(loaded.zoom_level, 15);
        assert!(loaded.dnd);
        assert_eq!(loaded.gpu_preference, GpuPreference::Discrete);
        assert_eq!(loaded.theme_preference, ThemePreference::Dark);
        assert!(!loaded.auto_check_updates);
        assert_eq!(loaded.last_update_check_unix, 1_700_000_000);
    }

    #[test]
    fn defaults_on_corrupt_json() {
        let dir = std::env::temp_dir().join("slackinux_settings_corrupt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "not json").unwrap();

        let settings = Settings::load(&dir);
        assert_eq!(settings.zoom_level, 10);
    }

    #[test]
    fn missing_gpu_field_defaults_to_auto() {
        let dir = std::env::temp_dir().join("slackinux_settings_old_format");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"zoom_level": 12, "dnd": true}"#,
        )
        .unwrap();

        let settings = Settings::load(&dir);
        assert_eq!(settings.zoom_level, 12);
        assert!(settings.dnd);
        assert_eq!(settings.gpu_preference, GpuPreference::Auto);
        assert_eq!(settings.theme_preference, ThemePreference::System);
        assert!(settings.auto_check_updates);
    }

    #[test]
    fn update_fields_default_when_absent() {
        let dir = std::env::temp_dir().join("slackinux_settings_no_updater_fields");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"zoom_level": 10, "dnd": false, "gpu_preference": "auto"}"#,
        )
        .unwrap();

        let settings = Settings::load(&dir);
        assert!(settings.auto_check_updates);
        assert_eq!(settings.last_update_check_unix, 0);
        assert_eq!(settings.theme_preference, ThemePreference::System);
    }
}

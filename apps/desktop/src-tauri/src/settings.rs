use std::fmt;

use serde::{Deserialize, Serialize};

/// How Slackinux steers WebKit's graphics stack. The default is `Automatic`,
/// which uses the compositor/system-selected GPU and keeps hardware
/// acceleration enabled. Software rendering is never chosen implicitly by GPU
/// vendor or session type; it requires an explicit user choice or a confirmed,
/// repeated rendering failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsMode {
    /// Use the compositor/system-selected GPU; keep hardware acceleration.
    #[default]
    Automatic,
    /// Prefer the integrated GPU where the app can validly steer rendering
    /// (X11 PRIME). On Wayland the compositor selects the GPU.
    Efficient,
    /// Prefer the discrete GPU when explicitly selected (X11 PRIME offload).
    Performance,
    /// Keep hardware acceleration but disable unstable paths such as DMABUF.
    Compatibility,
    /// Disable accelerated compositing explicitly.
    Software,
}

impl fmt::Display for GraphicsMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphicsMode::Automatic => write!(f, "automatic"),
            GraphicsMode::Efficient => write!(f, "efficient"),
            GraphicsMode::Performance => write!(f, "performance"),
            GraphicsMode::Compatibility => write!(f, "compatibility"),
            GraphicsMode::Software => write!(f, "software"),
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
    pub graphics_mode: GraphicsMode,
    /// Legacy field retained so older settings files migrate their GPU choice.
    #[serde(default)]
    pub gpu_preference: Option<LegacyGpuPreference>,
    #[serde(default)]
    pub theme_preference: ThemePreference,
    #[serde(default = "default_auto_check_updates")]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub last_update_check_unix: i64,
}

/// The pre-0.4 GPU preference values, kept only for migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyGpuPreference {
    Auto,
    Integrated,
    Discrete,
}

impl From<LegacyGpuPreference> for GraphicsMode {
    fn from(value: LegacyGpuPreference) -> Self {
        match value {
            LegacyGpuPreference::Auto => GraphicsMode::Automatic,
            LegacyGpuPreference::Integrated => GraphicsMode::Efficient,
            LegacyGpuPreference::Discrete => GraphicsMode::Performance,
        }
    }
}

fn default_auto_check_updates() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zoom_level: 10,
            dnd: false,
            graphics_mode: GraphicsMode::Automatic,
            gpu_preference: None,
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
            Ok(content) => {
                let mut settings = serde_json::from_str::<Settings>(&content).unwrap_or_default();
                // Backward-compatible migration: an older file may have a
                // `gpu_preference` but no `graphics_mode`. Fold the legacy
                // choice in rather than silently dropping it.
                if settings.graphics_mode == GraphicsMode::Automatic {
                    if let Some(legacy) = settings.gpu_preference {
                        settings.graphics_mode = legacy.into();
                    }
                }
                settings
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, data_dir: &std::path::Path) {
        let path = data_dir.join("settings.json");
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let temporary = data_dir.join("settings.json.tmp");
            let result = std::fs::write(&temporary, content)
                .and_then(|_| std::fs::rename(&temporary, &path));
            if let Err(error) = result {
                log::warn!("could not save settings atomically: {error}");
                let _ = std::fs::remove_file(temporary);
            }
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
        assert_eq!(settings.graphics_mode, GraphicsMode::Automatic);
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
            graphics_mode: GraphicsMode::Performance,
            gpu_preference: None,
            theme_preference: ThemePreference::Dark,
            auto_check_updates: false,
            last_update_check_unix: 1_700_000_000,
        };
        settings.save(&dir);

        let loaded = Settings::load(&dir);
        assert_eq!(loaded.zoom_level, 15);
        assert!(loaded.dnd);
        assert_eq!(loaded.graphics_mode, GraphicsMode::Performance);
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
    fn missing_graphics_mode_defaults_to_automatic() {
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
        assert_eq!(settings.graphics_mode, GraphicsMode::Automatic);
        assert_eq!(settings.theme_preference, ThemePreference::System);
        assert!(settings.auto_check_updates);
    }

    #[test]
    fn legacy_gpu_preference_migrates_to_graphics_mode() {
        let dir = std::env::temp_dir().join("slackinux_settings_legacy_gpu");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"zoom_level": 10, "dnd": false, "gpu_preference": "integrated"}"#,
        )
        .unwrap();

        let settings = Settings::load(&dir);
        assert_eq!(settings.graphics_mode, GraphicsMode::Efficient);

        std::fs::write(
            dir.join("settings.json"),
            r#"{"zoom_level": 10, "dnd": false, "gpu_preference": "discrete"}"#,
        )
        .unwrap();
        let settings = Settings::load(&dir);
        assert_eq!(settings.graphics_mode, GraphicsMode::Performance);

        std::fs::write(
            dir.join("settings.json"),
            r#"{"zoom_level": 10, "dnd": false, "gpu_preference": "auto"}"#,
        )
        .unwrap();
        let settings = Settings::load(&dir);
        assert_eq!(settings.graphics_mode, GraphicsMode::Automatic);
    }

    #[test]
    fn explicit_graphics_mode_wins_over_legacy_field() {
        let dir = std::env::temp_dir().join("slackinux_settings_graphics_wins");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"zoom_level": 10, "dnd": false, "graphics_mode": "software", "gpu_preference": "auto"}"#,
        )
        .unwrap();

        let settings = Settings::load(&dir);
        assert_eq!(settings.graphics_mode, GraphicsMode::Software);
    }

    #[test]
    fn update_fields_default_when_absent() {
        let dir = std::env::temp_dir().join("slackinux_settings_no_updater_fields");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"zoom_level": 10, "dnd": false, "graphics_mode": "automatic"}"#,
        )
        .unwrap();

        let settings = Settings::load(&dir);
        assert!(settings.auto_check_updates);
        assert_eq!(settings.last_update_check_unix, 0);
        assert_eq!(settings.theme_preference, ThemePreference::System);
    }
}

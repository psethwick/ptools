use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Persistent user settings stored in `~/.pterry/settings.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Optional GitHub personal-access token.  Authenticates GitHub API
    /// requests (e.g. downloading Raycast extensions) and raises the rate
    /// limit from 60 to 5 000 requests per hour.
    #[serde(default)]
    pub github_token: Option<String>,
    /// UI scale factor (pixels per point).  `None` uses the OS native DPI
    /// scale reported by the window system.  Set to e.g. `1.5` to override.
    #[serde(default)]
    pub pixels_per_point: Option<f32>,
    /// Color theme: `"dark"` | `"light"`.  `None` defaults to dark.
    #[serde(default)]
    pub theme: Option<String>,
    /// Global hotkey string for the main toggle-window action.
    /// `None` defaults to `"alt+space"`.  Accepts the same format as the
    /// `global-hotkey` crate (e.g. `"ctrl+space"`, `"super+space"`).
    #[serde(default)]
    pub toggle_hotkey: Option<String>,
    /// Per-extension launch hotkeys.  Keys are extension mode names (e.g.
    /// `"calculator"`); values are hotkey strings (e.g. `"ctrl+shift+c"`).
    /// When pressed the launcher opens directly into that extension's mode.
    #[serde(default)]
    pub extension_hotkeys: HashMap<String, String>,
}

/// Return the egui [`Visuals`] that correspond to the given theme string.
///
/// Delegates to [`crate::theme::visuals_for_theme`] which applies the full
/// spec-defined colour palette.  Accepts `"light"`, `"dark"`, or `"system"`
/// (follows OS preference); anything else (including `None`) produces dark.
pub fn visuals_for_theme(theme: Option<&str>) -> egui::Visuals {
    crate::theme::visuals_for_theme(theme)
}

impl Settings {
    /// Load settings from disk.  Returns [`Settings::default`] if the file
    /// does not exist or cannot be parsed.
    pub fn load() -> Self {
        let path = Self::path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist settings to [`Self::path`].
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Canonical path for the settings file: `~/.pterry/settings.json`.
    pub fn path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".pterry")
            .join("settings.json")
    }

    /// Build a `ureq` GET request for a GitHub API URL, injecting the stored
    /// token as an `Authorization` header when one is set.  Pass the returned
    /// request to `.call()` at the call-site.
    pub fn github_request(url: &str) -> ureq::Request {
        let req = ureq::get(url);
        let settings = Self::load();
        match settings.github_token {
            Some(token) => req.set("Authorization", &format!("Bearer {token}")),
            None => req,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_has_no_token() {
        assert!(Settings::default().github_token.is_none());
    }

    #[test]
    fn settings_path_ends_with_settings_json() {
        let p = Settings::path();
        assert!(p.to_string_lossy().ends_with("settings.json"));
    }

    #[test]
    fn settings_path_contains_raycast_clone_dir() {
        let p = Settings::path();
        assert!(p.to_string_lossy().contains(".pterry"));
    }

    #[test]
    fn settings_serializes_with_github_token() {
        let s = Settings {
            github_token: Some("ghp_abc".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("ghp_abc"));
    }

    #[test]
    fn settings_deserializes_github_token() {
        let json = r#"{"github_token": "ghp_xyz"}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.github_token.as_deref(), Some("ghp_xyz"));
    }

    #[test]
    fn settings_round_trip_preserves_none_token() {
        let s = Settings {
            github_token: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert!(loaded.github_token.is_none());
    }

    #[test]
    fn settings_round_trip_preserves_token() {
        let s = Settings {
            github_token: Some("ghp_testtoken123".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.github_token.as_deref(), Some("ghp_testtoken123"));
    }

    #[test]
    fn settings_load_missing_field_returns_none() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(s.github_token.is_none());
    }

    #[test]
    fn default_settings_has_no_pixels_per_point() {
        assert!(Settings::default().pixels_per_point.is_none());
    }

    #[test]
    fn settings_round_trip_preserves_pixels_per_point() {
        let s = Settings {
            pixels_per_point: Some(1.5),
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.pixels_per_point, Some(1.5));
    }

    #[test]
    fn settings_missing_pixels_per_point_deserializes_to_none() {
        let s: Settings = serde_json::from_str(r#"{"github_token": null}"#).unwrap();
        assert!(s.pixels_per_point.is_none());
    }

    // --- theme field ---

    #[test]
    fn default_settings_has_no_theme() {
        assert!(Settings::default().theme.is_none());
    }

    #[test]
    fn settings_round_trip_preserves_theme_light() {
        let s = Settings {
            theme: Some("light".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.theme.as_deref(), Some("light"));
    }

    #[test]
    fn settings_missing_theme_deserializes_to_none() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(s.theme.is_none());
    }

    // --- visuals_for_theme ---

    #[test]
    fn visuals_none_gives_dark() {
        let v = visuals_for_theme(None);
        assert!(v.dark_mode);
    }

    #[test]
    fn visuals_dark_gives_dark() {
        let v = visuals_for_theme(Some("dark"));
        assert!(v.dark_mode);
    }

    #[test]
    fn visuals_light_gives_light() {
        let v = visuals_for_theme(Some("light"));
        assert!(!v.dark_mode);
    }

    #[test]
    fn visuals_unknown_falls_back_to_dark() {
        let v = visuals_for_theme(Some("banana"));
        assert!(v.dark_mode);
    }
}

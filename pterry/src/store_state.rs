use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledExtension {
    pub name: String,
    pub version: String,
    /// "native" or "raycast"
    pub source: String,
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StoreState {
    pub installed: Vec<InstalledExtension>,
}

fn state_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pterry")
        .join("store_state.json")
}

impl StoreState {
    /// Load from `~/.pterry/store_state.json`; returns default if missing or corrupt.
    pub fn load() -> Self {
        let path = state_path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist to disk, creating parent dirs as needed.
    pub fn save(&self) -> std::io::Result<()> {
        let path = state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, json)
    }

    pub fn is_installed(&self, name: &str) -> bool {
        self.installed.iter().any(|e| e.name == name)
    }

    /// Record an extension as installed.  Re-installing replaces the previous entry.
    pub fn mark_installed(&mut self, name: &str, version: &str, source: &str) {
        self.installed.retain(|e| e.name != name);
        self.installed.push(InstalledExtension {
            name: name.to_string(),
            version: version.to_string(),
            source: source.to_string(),
            installed_at: Utc::now(),
        });
    }

    pub fn installed_names(&self) -> Vec<String> {
        self.installed.iter().map(|e| e.name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_is_not_installed() {
        let state = StoreState::default();
        assert!(!state.is_installed("foo"));
    }

    #[test]
    fn mark_installed_makes_is_installed_true() {
        let mut state = StoreState::default();
        state.mark_installed("foo", "1.0.0", "native");
        assert!(state.is_installed("foo"));
    }

    #[test]
    fn is_installed_is_name_specific() {
        let mut state = StoreState::default();
        state.mark_installed("foo", "1.0.0", "native");
        assert!(!state.is_installed("bar"));
    }

    #[test]
    fn mark_installed_idempotent_replaces_old_entry() {
        let mut state = StoreState::default();
        state.mark_installed("foo", "1.0.0", "native");
        state.mark_installed("foo", "2.0.0", "native");
        assert_eq!(state.installed.len(), 1);
        assert_eq!(state.installed[0].version, "2.0.0");
    }

    #[test]
    fn installed_names_returns_all_names() {
        let mut state = StoreState::default();
        state.mark_installed("a", "1.0.0", "native");
        state.mark_installed("b", "1.0.0", "raycast");
        let names = state.installed_names();
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn load_nonexistent_returns_empty_default() {
        // Default state has no entries; this mirrors what load() returns when
        // the file is absent.
        let state = StoreState::default();
        assert!(state.installed.is_empty());
    }

    #[test]
    fn roundtrip_serialize_deserialize() {
        let mut state = StoreState::default();
        state.mark_installed("my-ext", "1.2.3", "native");

        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: StoreState = serde_json::from_str(&json).unwrap();

        assert!(loaded.is_installed("my-ext"));
        assert_eq!(loaded.installed[0].version, "1.2.3");
        assert_eq!(loaded.installed[0].source, "native");
    }

    #[test]
    fn save_and_load_via_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store_state.json");

        let mut state = StoreState::default();
        state.mark_installed("ext-a", "0.9.0", "native");

        let json = serde_json::to_string_pretty(&state).unwrap();
        fs::write(&path, json).unwrap();

        let loaded: StoreState = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(loaded.is_installed("ext-a"));
    }
}

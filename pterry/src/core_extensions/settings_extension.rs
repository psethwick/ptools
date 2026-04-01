use crate::extension_trait::{
    Extension, ExtensionError, ExtensionItem, ExtensionLanguage, ExtensionMetadata, ExtraAction,
    PreferenceSpec,
};
use crate::modes;
use crate::settings::Settings;
use async_trait::async_trait;
use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

// ── SettingsExtension ────────────────────────────────────────────────────────

/// Top-level Settings portal.  `auto_load: false` keeps it out of the global
/// broadcast; `launcher_item()` lets users discover it by typing "settings".
/// Selecting the launcher item enters `settings` mode, which shows individual
/// setting controls.
pub struct SettingsExtension {
    metadata: ExtensionMetadata,
}

impl fmt::Debug for SettingsExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl SettingsExtension {
    pub fn new() -> Self {
        let metadata = ExtensionMetadata {
            name: modes::SETTINGS.to_string(),
            version: "1.0.0".to_string(),
            description: Some("Configure launcher settings".to_string()),
            author: None,
            language: ExtensionLanguage::Rust,
            entry_point: "settings_extension.rs".to_string(),
            permissions: vec![],
            auto_load: false,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        Self { metadata }
    }
}

impl Default for SettingsExtension {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the list of setting items filtered by `query`.
/// Pure function — reads settings from disk on each call so the subtitle
/// always reflects the current token state.
pub fn settings_items(query: &str) -> Vec<ExtensionItem> {
    let settings = Settings::load();
    let token_subtitle = match &settings.github_token {
        Some(t) => {
            let preview: String = t.chars().take(4).collect();
            format!("Set ({preview}…)")
        }
        None => "Not set — rate-limited to 60 req/hr".to_string(),
    };

    let current_theme = settings.theme.as_deref().unwrap_or("dark");
    let (theme_subtitle, toggle_theme) = if current_theme == "light" {
        ("Light", "dark")
    } else {
        ("Dark", "light")
    };

    let all = vec![
        ExtensionItem {
            title: "Extension Preferences".to_string(),
            subtitle: Some("Configure preferences for installed extensions".to_string()),
            icon: Some("🧩".to_string()),
            action: format!("enter-mode:{}", modes::SETTINGS_EXT_PREFS),
            id: Some("ext-prefs".to_string()),
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        },
        ExtensionItem {
            title: "GitHub Token".to_string(),
            subtitle: Some(token_subtitle),
            icon: Some("🔑".to_string()),
            action: format!("enter-mode:{}", modes::SETTINGS_SET_GITHUB_TOKEN),
            id: Some("github-token".to_string()),
            detail: Some(
                "## GitHub API Token\n\nAn optional personal-access token that raises the \
                 GitHub API rate limit from 60 to 5 000 requests per hour.\n\nUsed when \
                 installing Raycast extensions from the store."
                    .to_string(),
            ),
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        },
        ExtensionItem {
            title: format!("Theme: {theme_subtitle}"),
            subtitle: Some(format!("Switch to {toggle_theme} mode")),
            icon: Some("🎨".to_string()),
            action: format!("settings-set-theme:{toggle_theme}"),
            id: Some("theme".to_string()),
            detail: None,
            accessories: vec![],
            extra_actions: vec![
                ExtraAction {
                    title: "Set Dark".to_string(),
                    action: "settings-set-theme:dark".to_string(),
                    icon: None,
                    shortcut: None,
                },
                ExtraAction {
                    title: "Set Light".to_string(),
                    action: "settings-set-theme:light".to_string(),
                    icon: None,
                    shortcut: None,
                },
            ],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        },
    ];

    if query.is_empty() {
        return all;
    }
    let q = query.to_lowercase();
    all.into_iter()
        .filter(|item| {
            item.title.to_lowercase().contains(&q)
                || item
                    .subtitle
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
        })
        .collect()
}

#[async_trait]
impl Extension for SettingsExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        Ok(settings_items(query))
    }

    async fn on_action(&self, _action: &str, _item_id: Option<&str>) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }

    fn launcher_item(&self) -> Option<ExtensionItem> {
        Some(ExtensionItem {
            title: "Settings".to_string(),
            subtitle: Some("Configure launcher settings".to_string()),
            icon: Some("⚙️".to_string()),
            action: format!("enter-mode:{}", modes::SETTINGS),
            id: Some("settings-launcher".to_string()),
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        })
    }
}

// ── SetGitHubTokenExtension ──────────────────────────────────────────────────

/// Dedicated input mode for setting the GitHub API token.  The search box acts
/// as a text field: whatever the user types becomes the new token value.
/// Pressing Enter saves it; Escape cancels without saving.
pub struct SetGitHubTokenExtension {
    metadata: ExtensionMetadata,
}

impl fmt::Debug for SetGitHubTokenExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetGitHubTokenExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl SetGitHubTokenExtension {
    pub fn new() -> Self {
        let metadata = ExtensionMetadata {
            name: modes::SETTINGS_SET_GITHUB_TOKEN.to_string(),
            version: "1.0.0".to_string(),
            description: Some("Enter your GitHub personal-access token".to_string()),
            author: None,
            language: ExtensionLanguage::Rust,
            entry_point: "settings_extension.rs".to_string(),
            permissions: vec![],
            auto_load: false,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        Self { metadata }
    }
}

impl Default for SetGitHubTokenExtension {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the single "set token" item from the current search `query`.
/// Pure function for testability.
pub fn set_token_items(query: &str) -> Vec<ExtensionItem> {
    if query.is_empty() {
        vec![ExtensionItem {
            title: "GitHub Token".to_string(),
            subtitle: Some("Type your token and press Enter to save (Esc to cancel)".to_string()),
            icon: Some("🔑".to_string()),
            action: "settings-set-github-token:save:".to_string(),
            id: None,
            detail: Some(
                "## Set GitHub Token\n\nType your GitHub personal-access token in the \
                 search box above and press **Enter** to save it.\n\n\
                 Generate one at: https://github.com/settings/tokens\n\n\
                 Leave the box empty and press Enter to clear the stored token."
                    .to_string(),
            ),
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        }]
    } else {
        vec![ExtensionItem {
            title: format!("Set GitHub Token: {query}"),
            subtitle: Some("Press Enter to save".to_string()),
            icon: Some("🔑".to_string()),
            action: format!("settings-set-github-token:save:{query}"),
            id: None,
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        }]
    }
}

#[async_trait]
impl Extension for SetGitHubTokenExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        Ok(set_token_items(query))
    }

    async fn on_action(&self, action: &str, item_id: Option<&str>) -> Result<(), ExtensionError> {
        if !action.starts_with("save:") {
            return Ok(());
        }
        // `item_id` carries everything after the second colon in the original
        // action string, which is the token value (may be empty to clear).
        let token = item_id.unwrap_or("").trim();
        let mut settings = Settings::load();
        settings.github_token = if token.is_empty() {
            None
        } else {
            Some(token.to_string())
        };
        settings
            .save()
            .map_err(|e| ExtensionError::ExecutionError(format!("Failed to save settings: {e}")))?;
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }
}

// ── ExtensionPrefsExtension ──────────────────────────────────────────────────

/// Scan `~/.pterry/extensions/` and `./extensions/` for extension.json
/// files that declare a non-empty `"preferences"` array.
/// Returns `(name, description, preferences)` for each matching extension.
pub fn scan_extensions_with_prefs() -> Vec<(String, Option<String>, Vec<PreferenceSpec>)> {
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let user_dir = home_dir.join(".pterry").join("extensions");
    let dev_dir = PathBuf::from("extensions");

    let mut results: Vec<(String, Option<String>, Vec<PreferenceSpec>)> = vec![];
    let mut seen: HashSet<String> = HashSet::new();

    for dir in [&user_dir, &dev_dir] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();

            // Determine the extension name and which JSON to read.
            let (ext_name, json_path) = if path.is_dir() {
                // Package-style: <name>/extension.json
                let Some(name) = path.file_name().and_then(|s| s.to_str()).map(|s| s.to_string())
                else {
                    continue;
                };
                let json = path.join("extension.json");
                if !json.exists() {
                    continue;
                }
                (name, json)
            } else if path.is_file() {
                // Single-file extension: <name>.js / .ts / .tsx with a sidecar <name>.json
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    continue; // skip JSON files themselves
                }
                let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
                else {
                    continue;
                };
                let json = path.with_extension("json");
                if !json.exists() {
                    continue;
                }
                (name, json)
            } else {
                continue;
            };

            if seen.contains(&ext_name) {
                continue;
            }

            let Ok(content) = std::fs::read_to_string(&json_path) else {
                continue;
            };
            let Ok(meta) = serde_json::from_str::<ExtensionMetadata>(&content) else {
                continue;
            };

            if !meta.preferences.is_empty() {
                seen.insert(ext_name.clone());
                results.push((ext_name, meta.description, meta.preferences));
            }
        }
    }

    results
}

/// Save preference values for the named extension to
/// `~/.pterry/extension-data/<ext_name>/preferences.json`.
pub fn save_extension_prefs(
    ext_name: &str,
    values: &std::collections::HashMap<String, String>,
) -> Result<(), std::io::Error> {
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let prefs_dir = home_dir
        .join(".pterry")
        .join("extension-data")
        .join(ext_name);
    std::fs::create_dir_all(&prefs_dir)?;
    let json = serde_json::to_string_pretty(values)
        .map_err(std::io::Error::other)?;
    std::fs::write(prefs_dir.join("preferences.json"), json)?;
    Ok(())
}

/// Build the list of extensions that have preferences, filtered by `query`.
/// Each item's action is `settings-show-ext-prefs:<name>` which tells
/// `execute_action` in `app.rs` to open the preference form for that extension.
pub fn ext_prefs_items(query: &str) -> Vec<ExtensionItem> {
    let extensions = scan_extensions_with_prefs();
    let q = query.to_lowercase();

    extensions
        .into_iter()
        .filter(|(name, desc, _)| {
            if q.is_empty() {
                return true;
            }
            name.to_lowercase().contains(&q)
                || desc
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
        })
        .map(|(name, description, prefs)| ExtensionItem {
            title: name.clone(),
            subtitle: description
                .or_else(|| Some(format!("{} preference(s)", prefs.len()))),
            icon: Some("⚙️".to_string()),
            action: format!("settings-show-ext-prefs:{name}"),
            id: Some(name.clone()),
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        })
        .collect()
}

/// Lists installed extensions that expose preferences, so the user can
/// configure them from the Settings → Extension Preferences sub-mode.
pub struct ExtensionPrefsExtension {
    metadata: ExtensionMetadata,
}

impl fmt::Debug for ExtensionPrefsExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtensionPrefsExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl ExtensionPrefsExtension {
    pub fn new() -> Self {
        let metadata = ExtensionMetadata {
            name: modes::SETTINGS_EXT_PREFS.to_string(),
            version: "1.0.0".to_string(),
            description: Some("Configure extension preferences".to_string()),
            author: None,
            language: ExtensionLanguage::Rust,
            entry_point: "settings_extension.rs".to_string(),
            permissions: vec![],
            auto_load: false,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        Self { metadata }
    }
}

impl Default for ExtensionPrefsExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for ExtensionPrefsExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        Ok(ext_prefs_items(query))
    }

    async fn on_action(&self, _action: &str, _item_id: Option<&str>) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SettingsExtension ─────────────────────────────────────────────────────

    #[test]
    fn settings_extension_not_auto_loaded() {
        assert!(!SettingsExtension::new().metadata().auto_load);
    }

    #[test]
    fn settings_extension_correct_name() {
        assert_eq!(
            SettingsExtension::new().metadata().name,
            crate::modes::SETTINGS
        );
    }

    #[test]
    fn settings_extension_has_launcher_item() {
        assert!(SettingsExtension::new().launcher_item().is_some());
    }

    #[test]
    fn launcher_item_enters_settings_mode() {
        let item = SettingsExtension::new().launcher_item().unwrap();
        assert_eq!(
            item.action,
            format!("enter-mode:{}", crate::modes::SETTINGS)
        );
    }

    #[test]
    fn settings_items_empty_query_returns_two_items() {
        let items = settings_items("");
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn settings_items_empty_query_includes_github_token_item() {
        let items = settings_items("");
        assert!(items.iter().any(|i| i.title == "GitHub Token"));
    }

    #[test]
    fn settings_items_empty_query_includes_theme_item() {
        let items = settings_items("");
        assert!(items.iter().any(|i| i.title.contains("Theme")));
    }

    #[test]
    fn settings_items_theme_query_matches() {
        let items = settings_items("theme");
        assert!(!items.is_empty());
        assert!(items[0].title.contains("Theme"));
    }

    #[test]
    fn settings_items_dark_theme_action_sets_light() {
        // When current theme is dark (None), the toggle should offer switching to light.
        let items = settings_items("");
        let theme_item = items.iter().find(|i| i.title.contains("Theme")).unwrap();
        // One of the extra_actions or the primary action should be settings-set-theme:*
        let all_actions: Vec<&str> = std::iter::once(theme_item.action.as_str())
            .chain(theme_item.extra_actions.iter().map(|a| a.action.as_str()))
            .collect();
        assert!(
            all_actions
                .iter()
                .any(|a| a.starts_with("settings-set-theme:"))
        );
    }

    #[test]
    fn settings_items_github_query_matches() {
        let items = settings_items("github");
        assert!(!items.is_empty());
    }

    #[test]
    fn settings_items_unmatched_query_returns_empty() {
        let items = settings_items("zzznomatch");
        assert!(items.is_empty());
    }

    #[test]
    fn settings_item_action_enters_token_mode() {
        let items = settings_items("");
        let token_item = items
            .iter()
            .find(|i| i.title == "GitHub Token")
            .expect("GitHub Token item must exist");
        assert_eq!(
            token_item.action,
            format!("enter-mode:{}", crate::modes::SETTINGS_SET_GITHUB_TOKEN)
        );
    }

    // ── SetGitHubTokenExtension ───────────────────────────────────────────────

    #[test]
    fn set_token_extension_not_auto_loaded() {
        assert!(!SetGitHubTokenExtension::new().metadata().auto_load);
    }

    #[test]
    fn set_token_extension_correct_name() {
        assert_eq!(
            SetGitHubTokenExtension::new().metadata().name,
            crate::modes::SETTINGS_SET_GITHUB_TOKEN
        );
    }

    #[test]
    fn set_token_empty_query_returns_one_item() {
        let items = set_token_items("");
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn set_token_empty_query_has_placeholder_subtitle() {
        let items = set_token_items("");
        let subtitle = items[0].subtitle.as_deref().unwrap_or("");
        assert!(subtitle.contains("Type your token"));
    }

    #[test]
    fn set_token_nonempty_query_includes_query_in_title() {
        let items = set_token_items("ghp_abc123");
        assert!(items[0].title.contains("ghp_abc123"));
    }

    #[test]
    fn set_token_action_includes_save_prefix() {
        let items = set_token_items("ghp_test");
        assert!(items[0].action.contains("save:"));
        assert!(items[0].action.contains("ghp_test"));
    }

    #[test]
    fn set_token_empty_query_action_has_save_prefix() {
        let items = set_token_items("");
        assert!(
            items[0]
                .action
                .starts_with("settings-set-github-token:save:")
        );
    }
}

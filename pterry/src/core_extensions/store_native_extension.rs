use crate::extension_manager::ExtensionMessage;
use crate::extension_trait::{
    Extension, ExtensionError, ExtensionItem, ExtensionLanguage, ExtensionMetadata,
};
use crate::modes;
use crate::store_state::StoreState;
use async_trait::async_trait;
use crossbeam_channel::Sender;
use serde::Deserialize;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// URL of the native extension registry manifest.
const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/your-org/raycast-clone-extensions/main/registry.json";

/// How long cached registry data stays fresh.
const CACHE_TTL: Duration = Duration::from_secs(300);

/// One entry in the native registry manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct RegistryEntry {
    pub name: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub source_url: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

struct Cache {
    entries: Vec<RegistryEntry>,
    fetched_at: Instant,
}

pub struct StoreNativeExtension {
    metadata: ExtensionMetadata,
    cache: Arc<RwLock<Option<Cache>>>,
    registry_url: String,
    sender: Option<Sender<ExtensionMessage>>,
}

impl fmt::Debug for StoreNativeExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreNativeExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl StoreNativeExtension {
    pub fn new() -> Self {
        Self::with_url(REGISTRY_URL.to_string(), None)
    }

    pub fn new_with_sender(sender: Sender<ExtensionMessage>) -> Self {
        Self::with_url(REGISTRY_URL.to_string(), Some(sender))
    }

    fn with_url(registry_url: String, sender: Option<Sender<ExtensionMessage>>) -> Self {
        let metadata = ExtensionMetadata {
            name: modes::STORE_NATIVE.to_string(),
            version: "1.0.0".to_string(),
            description: Some("Browse native extensions".to_string()),
            author: None,
            language: ExtensionLanguage::Rust,
            entry_point: "store_native_extension.rs".to_string(),
            permissions: vec!["network".to_string()],
            auto_load: false,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        Self {
            metadata,
            cache: Arc::new(RwLock::new(None)),
            registry_url,
            sender,
        }
    }

    async fn get_entries(&self) -> Vec<RegistryEntry> {
        {
            let guard = self.cache.read().await;
            if let Some(ref c) = *guard
                && c.fetched_at.elapsed() < CACHE_TTL
            {
                return c.entries.clone();
            }
        }

        let url = self.registry_url.clone();
        let entries = tokio::task::spawn_blocking(move || {
            ureq::get(&url)
                .call()
                .ok()
                .and_then(|resp| resp.into_string().ok())
                .and_then(|body| serde_json::from_str::<Vec<RegistryEntry>>(&body).ok())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

        *self.cache.write().await = Some(Cache {
            entries: entries.clone(),
            fetched_at: Instant::now(),
        });
        entries
    }

    fn send_toast(&self, style: &str, title: &str, message: &str) {
        if let Some(ref sender) = self.sender {
            let _ = sender.send(ExtensionMessage::ShowToast(
                style.to_string(),
                title.to_string(),
                message.to_string(),
            ));
        }
    }
}

impl Default for StoreNativeExtension {
    fn default() -> Self {
        Self::new()
    }
}

/// Derive the file extension to use when writing the downloaded source to disk.
/// Falls back to "js" when the URL has no recognisable extension.
fn source_file_ext(source_url: &str) -> &str {
    source_url
        .rsplit('.')
        .next()
        .filter(|e| matches!(*e, "js" | "ts" | "tsx"))
        .unwrap_or("js")
}

/// Filter native registry entries by query (case-insensitive substring on title, description,
/// author). Installed extensions show a checkmark prefix in their subtitle.
/// Extracted as a pure function for testability.
pub fn filter_native_items(
    entries: &[RegistryEntry],
    query: &str,
    installed: &[String],
) -> Vec<ExtensionItem> {
    let q = query.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            q.is_empty()
                || e.title.to_lowercase().contains(&q)
                || e.description.to_lowercase().contains(&q)
                || e.author.to_lowercase().contains(&q)
        })
        .map(|e| {
            let is_installed = installed.iter().any(|n| n == &e.name);
            let subtitle = if is_installed {
                format!("✓ Installed · by {} · v{}", e.author, e.version)
            } else {
                format!("by {} · v{}", e.author, e.version)
            };
            let icon = if is_installed {
                Some("✅".to_string())
            } else {
                Some("🔌".to_string())
            };
            ExtensionItem {
                title: e.title.clone(),
                subtitle: Some(subtitle),
                icon,
                action: format!("store-native:install:{}", e.name),
                id: Some(e.name.clone()),
                detail: Some(format!(
                    "## {}\n\n{}\n\n**Author:** {}\n**Version:** {}\n**Source:** {}",
                    e.title, e.description, e.author, e.version, e.source_url
                )),
                accessories: vec![],
                extra_actions: vec![],
                detail_metadata: vec![],
                thumbnail_rgba: None,
                grid_columns: None,
            }
        })
        .collect()
}

#[async_trait]
impl Extension for StoreNativeExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        let entries = self.get_entries().await;
        let state = StoreState::load();
        let installed = state.installed_names();
        Ok(filter_native_items(&entries, query, &installed))
    }

    async fn on_action(&self, action: &str, item_id: Option<&str>) -> Result<(), ExtensionError> {
        if action != "install" {
            return Ok(());
        }
        let name = match item_id {
            Some(n) => n.to_string(),
            None => return Ok(()),
        };

        // Find the registry entry for this extension.
        let entries = self.get_entries().await;
        let entry = match entries.iter().find(|e| e.name == name) {
            Some(e) => e.clone(),
            None => {
                self.send_toast(
                    "failure",
                    "Install Failed",
                    &format!("Extension '{name}' not found in registry"),
                );
                return Ok(());
            }
        };

        let ext_name = name.clone();
        let ext_version = entry.version.clone();
        let source_url = entry.source_url.clone();
        let file_ext = source_file_ext(&source_url).to_string();

        // Download + write inside spawn_blocking so we don't block the Tokio executor.
        let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let body = ureq::get(&source_url)
                .call()
                .map_err(|e| format!("Download failed: {e}"))?
                .into_string()
                .map_err(|e| format!("Read response failed: {e}"))?;

            let extensions_dir = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".pterry")
                .join("extensions");
            std::fs::create_dir_all(&extensions_dir)
                .map_err(|e| format!("Create dir failed: {e}"))?;

            let dest = extensions_dir.join(format!("{ext_name}.{file_ext}"));
            std::fs::write(&dest, body).map_err(|e| format!("Write failed: {e}"))?;

            // Persist install state.
            let mut state = StoreState::load();
            state.mark_installed(&ext_name, &ext_version, "native");
            state
                .save()
                .map_err(|e| format!("Save state failed: {e}"))?;

            Ok(())
        })
        .await
        .unwrap_or_else(|e| Err(format!("Task panicked: {e}")));

        match result {
            Ok(()) => {
                self.send_toast(
                    "success",
                    "Extension Installed",
                    &format!("'{}' installed — restart to load it", entry.title),
                );
            }
            Err(err) => {
                self.send_toast("failure", "Install Failed", &err);
            }
        }

        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(name: &str, title: &str, description: &str, author: &str) -> RegistryEntry {
        RegistryEntry {
            name: name.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            author: author.to_string(),
            version: "1.0.0".to_string(),
            source_url: format!("https://example.com/{name}.js"),
            permissions: vec![],
        }
    }

    #[test]
    fn empty_query_returns_all_entries() {
        let entries = vec![
            make_entry("a", "Awesome Extension", "Does awesome things", "alice"),
            make_entry("b", "Basic Tool", "Does basic things", "bob"),
        ];
        let items = filter_native_items(&entries, "", &[]);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn query_filters_by_title() {
        let entries = vec![
            make_entry("a", "Awesome Extension", "Does awesome things", "alice"),
            make_entry("b", "Basic Tool", "Does basic things", "bob"),
        ];
        let items = filter_native_items(&entries, "awesome", &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.as_deref(), Some("a"));
    }

    #[test]
    fn query_filters_by_description() {
        let entries = vec![
            make_entry("a", "Thing", "Has network support", "alice"),
            make_entry("b", "Other", "Has clipboard support", "bob"),
        ];
        let items = filter_native_items(&entries, "network", &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.as_deref(), Some("a"));
    }

    #[test]
    fn query_filters_by_author() {
        let entries = vec![
            make_entry("a", "Ext A", "desc a", "alice"),
            make_entry("b", "Ext B", "desc b", "bob"),
        ];
        let items = filter_native_items(&entries, "alice", &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.as_deref(), Some("a"));
    }

    #[test]
    fn query_is_case_insensitive() {
        let entries = vec![make_entry("a", "My Extension", "desc", "author")];
        let upper = filter_native_items(&entries, "EXTENSION", &[]);
        let lower = filter_native_items(&entries, "extension", &[]);
        assert_eq!(upper.len(), lower.len());
        assert_eq!(upper.len(), 1);
    }

    #[test]
    fn unmatched_query_returns_empty() {
        let entries = vec![make_entry("a", "My Extension", "desc", "author")];
        let items = filter_native_items(&entries, "zzznomatch", &[]);
        assert!(items.is_empty());
    }

    #[test]
    fn item_action_routes_to_store_native_extension() {
        let entries = vec![make_entry("my-ext", "My Ext", "desc", "author")];
        let items = filter_native_items(&entries, "", &[]);
        assert_eq!(items[0].action, "store-native:install:my-ext");
    }

    #[test]
    fn item_has_detail_with_all_metadata() {
        let entries = vec![make_entry("ext", "My Ext", "Great extension", "alice")];
        let items = filter_native_items(&entries, "", &[]);
        let detail = items[0].detail.as_deref().unwrap_or("");
        assert!(detail.contains("My Ext"));
        assert!(detail.contains("Great extension"));
        assert!(detail.contains("alice"));
    }

    #[test]
    fn store_native_not_auto_loaded() {
        let ext = StoreNativeExtension::new();
        assert!(!ext.metadata().auto_load);
    }

    #[test]
    fn store_native_has_correct_name() {
        let ext = StoreNativeExtension::new();
        assert_eq!(ext.metadata().name, crate::modes::STORE_NATIVE);
    }

    #[test]
    fn installed_extension_shows_checkmark_in_subtitle() {
        let entries = vec![make_entry("my-ext", "My Ext", "desc", "alice")];
        let installed = vec!["my-ext".to_string()];
        let items = filter_native_items(&entries, "", &installed);
        assert_eq!(items.len(), 1);
        let subtitle = items[0].subtitle.as_deref().unwrap_or("");
        assert!(
            subtitle.contains('✓'),
            "subtitle should contain checkmark: {subtitle}"
        );
    }

    #[test]
    fn not_installed_extension_has_plug_icon() {
        let entries = vec![make_entry("my-ext", "My Ext", "desc", "alice")];
        let items = filter_native_items(&entries, "", &[]);
        assert_eq!(items[0].icon.as_deref(), Some("🔌"));
    }

    #[test]
    fn installed_extension_has_check_icon() {
        let entries = vec![make_entry("my-ext", "My Ext", "desc", "alice")];
        let installed = vec!["my-ext".to_string()];
        let items = filter_native_items(&entries, "", &installed);
        assert_eq!(items[0].icon.as_deref(), Some("✅"));
    }

    #[test]
    fn source_file_ext_js() {
        assert_eq!(source_file_ext("https://example.com/ext.js"), "js");
    }

    #[test]
    fn source_file_ext_ts() {
        assert_eq!(source_file_ext("https://example.com/ext.ts"), "ts");
    }

    #[test]
    fn source_file_ext_tsx() {
        assert_eq!(source_file_ext("https://example.com/ext.tsx"), "tsx");
    }

    #[test]
    fn source_file_ext_unknown_falls_back_to_js() {
        assert_eq!(source_file_ext("https://example.com/ext.zip"), "js");
        assert_eq!(source_file_ext("https://example.com/ext"), "js");
    }
}

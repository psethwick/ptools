use crate::extension_trait::{
    Extension, ExtensionError, ExtensionItem, ExtensionLanguage, ExtensionMetadata,
};
use crate::modes;
use async_trait::async_trait;
use std::fmt;

pub struct StoreExtension {
    metadata: ExtensionMetadata,
}

impl fmt::Debug for StoreExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl Default for StoreExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl StoreExtension {
    pub fn new() -> Self {
        let metadata = ExtensionMetadata {
            name: modes::STORE.to_string(),
            version: "1.0.0".to_string(),
            description: Some("Browse and install extensions".to_string()),
            author: None,
            language: ExtensionLanguage::Rust,
            entry_point: "store_extension.rs".to_string(),
            permissions: vec!["network".to_string()],
            // Only active when explicitly entered via enter-mode:store.
            auto_load: false,
            title: None,
            preferences: vec![],
            is_development: false,
        };
        Self { metadata }
    }

    /// Returns the top-level catalog items (one per store tab), filtered by
    /// query.  An empty query returns all items.
    fn catalog_items(query: &str) -> Vec<ExtensionItem> {
        let tabs = [
            ExtensionItem {
                title: "Native Extensions".to_string(),
                subtitle: Some("Extensions built for this launcher".to_string()),
                icon: Some("🔌".to_string()),
                action: format!("store-tab:{}", modes::STORE_NATIVE),
                id: Some(modes::STORE_NATIVE.to_string()),
                detail: Some("Browse extensions from the native registry.".to_string()),
                accessories: vec![],
                extra_actions: vec![],
                detail_metadata: vec![],
                thumbnail_rgba: None,
                grid_columns: None,
            },
            ExtensionItem {
                title: "Raycast Store".to_string(),
                subtitle: Some("Compatible Raycast extensions".to_string()),
                icon: Some("⚡".to_string()),
                action: format!("store-tab:{}", modes::STORE_RAYCAST),
                id: Some(modes::STORE_RAYCAST.to_string()),
                detail: Some(
                    "Browse extensions from the official Raycast extension repository.".to_string(),
                ),
                accessories: vec![],
                extra_actions: vec![],
                detail_metadata: vec![],
                thumbnail_rgba: None,
                grid_columns: None,
            },
        ];

        if query.is_empty() {
            return tabs.into_iter().collect();
        }

        let q = query.to_lowercase();
        tabs.into_iter()
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
}

#[async_trait]
impl Extension for StoreExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        Ok(Self::catalog_items(query))
    }

    async fn on_action(&self, _action: &str, _item_id: Option<&str>) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }

    fn launcher_item(&self) -> Option<ExtensionItem> {
        Some(ExtensionItem {
            title: "Extension Store".to_string(),
            subtitle: Some("Browse and install extensions".to_string()),
            icon: Some("🏪".to_string()),
            action: format!("enter-mode:{}", modes::STORE),
            id: Some("store-launcher".to_string()),
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_extension_is_not_auto_loaded() {
        let ext = StoreExtension::new();
        assert!(!ext.metadata().auto_load);
    }

    #[test]
    fn store_extension_has_correct_name() {
        let ext = StoreExtension::new();
        assert_eq!(ext.metadata().name, crate::modes::STORE);
    }

    #[test]
    fn launcher_item_enters_store_mode() {
        let ext = StoreExtension::new();
        let item = ext
            .launcher_item()
            .expect("StoreExtension must have a launcher item");
        assert_eq!(item.action, format!("enter-mode:{}", crate::modes::STORE));
    }

    #[tokio::test]
    async fn empty_query_returns_both_tabs() {
        let ext = StoreExtension::new();
        let results = ext.on_search("").await.unwrap();
        assert_eq!(results.len(), 2);
        let actions: Vec<&str> = results.iter().map(|r| r.action.as_str()).collect();
        let native_action = format!("store-tab:{}", crate::modes::STORE_NATIVE);
        let raycast_action = format!("store-tab:{}", crate::modes::STORE_RAYCAST);
        assert!(actions.contains(&native_action.as_str()));
        assert!(actions.contains(&raycast_action.as_str()));
    }

    #[tokio::test]
    async fn query_native_returns_only_native_tab() {
        let ext = StoreExtension::new();
        let results = ext.on_search("native").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].action,
            format!("store-tab:{}", crate::modes::STORE_NATIVE)
        );
    }

    #[tokio::test]
    async fn query_raycast_returns_only_raycast_tab() {
        let ext = StoreExtension::new();
        let results = ext.on_search("raycast").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].action,
            format!("store-tab:{}", crate::modes::STORE_RAYCAST)
        );
    }

    #[tokio::test]
    async fn unmatched_query_returns_empty() {
        let ext = StoreExtension::new();
        let results = ext.on_search("xyznotfound").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn case_insensitive_filtering() {
        let ext = StoreExtension::new();
        let upper = ext.on_search("NATIVE").await.unwrap();
        let lower = ext.on_search("native").await.unwrap();
        assert_eq!(upper.len(), lower.len());
        assert_eq!(upper[0].action, lower[0].action);
    }
}

use crate::clipboard_manager::{ClipboardContent, ClipboardManager};
use crate::extension_trait::{Extension, ExtensionError, ExtensionItem, ExtensionMetadata};
use crate::fuzzy::fuzzy_match;
use crate::modes;
use async_trait::async_trait;
use std::fmt;
use std::sync::Arc;

pub struct ClipboardExtension {
    metadata: ExtensionMetadata,
    clipboard_manager: Arc<ClipboardManager>,
}

impl fmt::Debug for ClipboardExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClipboardExtension")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl ClipboardExtension {
    pub fn new(clipboard_manager: Arc<ClipboardManager>) -> Self {
        let metadata = ExtensionMetadata {
            name: modes::CLIPBOARD_HISTORY.to_string(),
            version: "1.0.0".to_string(),
            description: Some("System clipboard history manager".to_string()),
            author: None,
            language: crate::extension_trait::ExtensionLanguage::JavaScript,
            entry_point: "clipboard_extension.rs".to_string(),
            permissions: vec!["clipboard".to_string()],
            // Clipboard history must be explicitly activated; it must not appear
            // in the global broadcast search results.
            auto_load: false,
            title: None,
            preferences: vec![],
            is_development: false,
        };

        Self {
            metadata,
            clipboard_manager,
        }
    }

    fn format_timestamp(timestamp: &chrono::DateTime<chrono::Local>) -> String {
        let now = chrono::Local::now();
        let diff = now.signed_duration_since(*timestamp);

        if diff.num_seconds() < 60 {
            "Just now".to_string()
        } else if diff.num_minutes() < 60 {
            format!(
                "{} minute{} ago",
                diff.num_minutes(),
                if diff.num_minutes() > 1 { "s" } else { "" }
            )
        } else if diff.num_hours() < 24 {
            format!(
                "{} hour{} ago",
                diff.num_hours(),
                if diff.num_hours() > 1 { "s" } else { "" }
            )
        } else {
            format!(
                "{} day{} ago",
                diff.num_days(),
                if diff.num_days() > 1 { "s" } else { "" }
            )
        }
    }

    fn truncate_text(text: &str, max_length: usize) -> String {
        if text.len() <= max_length {
            text.to_string()
        } else {
            format!("{}...", &text[..max_length])
        }
    }
}

#[async_trait]
impl Extension for ClipboardExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn initialize(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
        let history = self.clipboard_manager.get_history();
        let mut items = Vec::new();

        for (index, item) in history.iter().enumerate() {
            match &item.content {
                ClipboardContent::Text(text) => {
                    if !query.is_empty() && !fuzzy_match(query, text) {
                        continue;
                    }
                    items.push(ExtensionItem {
                        title: Self::truncate_text(text, 80),
                        subtitle: Some(Self::format_timestamp(&item.timestamp)),
                        icon: Some("📋".to_string()),
                        action: format!("clipboard-paste:{index}"),
                        id: Some(format!("clipboard-{index}")),
                        detail: Some(text.clone()),
                        accessories: vec![],
                        extra_actions: vec![],
                        detail_metadata: vec![],
                        thumbnail_rgba: None,
                        grid_columns: None,
                    });
                }
                ClipboardContent::Image {
                    width,
                    height,
                    rgba,
                } => {
                    let search_str = format!("image {width}x{height}");
                    if !query.is_empty() && !fuzzy_match(query, &search_str) {
                        continue;
                    }
                    items.push(ExtensionItem {
                        title: format!("Image ({width}×{height})"),
                        subtitle: Some(Self::format_timestamp(&item.timestamp)),
                        icon: None,
                        action: format!("clipboard-paste:{index}"),
                        id: Some(format!("clipboard-img-{index}")),
                        detail: Some(format!(
                            "Image snapshot\nDimensions: {width} × {height} px\nCopied: {}",
                            Self::format_timestamp(&item.timestamp)
                        )),
                        accessories: vec![],
                        extra_actions: vec![],
                        detail_metadata: vec![],
                        thumbnail_rgba: Some((*width, *height, rgba.clone())),
                        grid_columns: None,
                    });
                }
            }
        }

        if items.is_empty() && !query.is_empty() {
            items.push(ExtensionItem {
                title: "No matching clipboard items".to_string(),
                subtitle: Some("Try a different search term".to_string()),
                icon: Some("🔍".to_string()),
                action: "clipboard-no-results".to_string(),
                id: None,
                detail: None,
                accessories: vec![],
                extra_actions: vec![],
                detail_metadata: vec![],
                thumbnail_rgba: None,
                grid_columns: None,
            });
        }

        Ok(items)
    }

    async fn on_action(&self, action: &str, _item_id: Option<&str>) -> Result<(), ExtensionError> {
        if action.starts_with("clipboard-paste:") {
            let index_str = action.strip_prefix("clipboard-paste:").unwrap();
            if let Ok(index) = index_str.parse::<usize>() {
                let history = self.clipboard_manager.get_history();
                if index < history.len() {
                    let item = &history[index];
                    match &item.content {
                        ClipboardContent::Text(text) => {
                            if let Err(e) = self.clipboard_manager.copy_to_clipboard(text) {
                                return Err(ExtensionError::ExecutionError(format!(
                                    "Failed to copy text: {e}"
                                )));
                            }
                            println!("Clipboard text pasted");
                        }
                        ClipboardContent::Image {
                            width,
                            height,
                            rgba,
                        } => {
                            if let Err(e) = self
                                .clipboard_manager
                                .copy_image_to_clipboard(*width, *height, rgba)
                            {
                                return Err(ExtensionError::ExecutionError(format!(
                                    "Failed to copy image: {e}"
                                )));
                            }
                            println!("Clipboard image pasted: {width}×{height}");
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), ExtensionError> {
        Ok(())
    }

    fn launcher_item(&self) -> Option<crate::extension_trait::ExtensionItem> {
        Some(crate::extension_trait::ExtensionItem {
            title: "Clipboard History".to_string(),
            subtitle: Some("Browse and paste recent clipboard entries".to_string()),
            icon: Some("📋".to_string()),
            action: format!("enter-mode:{}", modes::CLIPBOARD_HISTORY),
            id: Some(format!("{}-launcher", modes::CLIPBOARD_HISTORY)),
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
    use crate::clipboard_manager::{ClipboardContent, ClipboardItem, ClipboardManager};
    use chrono::Local;

    fn make_manager() -> Arc<ClipboardManager> {
        Arc::new(ClipboardManager::new().expect("ClipboardManager::new"))
    }

    fn text_item(text: &str) -> ClipboardItem {
        ClipboardItem {
            content: ClipboardContent::Text(text.to_string()),
            timestamp: Local::now(),
        }
    }

    fn image_item(w: usize, h: usize) -> ClipboardItem {
        ClipboardItem {
            content: ClipboardContent::Image {
                width: w,
                height: h,
                rgba: vec![0u8; w * h * 4],
            },
            timestamp: Local::now(),
        }
    }

    #[tokio::test]
    async fn text_item_detail_is_full_content() {
        let manager = make_manager();
        let long_text = "hello world this is a longer clipboard entry";
        manager.seed_history_for_test(vec![text_item(long_text)]);

        let ext = ClipboardExtension::new(manager);
        let results = ext.on_search("").await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].detail.as_deref(), Some(long_text));
    }

    #[tokio::test]
    async fn text_item_title_is_truncated_but_detail_is_not() {
        let manager = make_manager();
        // 100-char string — title truncates at 80, detail should be full
        let text: String = "x".repeat(100);
        manager.seed_history_for_test(vec![text_item(&text)]);

        let ext = ClipboardExtension::new(manager);
        let results = ext.on_search("").await.unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].title.len() < 100, "title should be truncated");
        assert_eq!(
            results[0].detail.as_deref(),
            Some(text.as_str()),
            "detail must be the full untruncated text"
        );
    }

    #[tokio::test]
    async fn image_item_detail_contains_dimensions() {
        let manager = make_manager();
        manager.seed_history_for_test(vec![image_item(800, 600)]);

        let ext = ClipboardExtension::new(manager);
        let results = ext.on_search("").await.unwrap();

        assert_eq!(results.len(), 1);
        let detail = results[0]
            .detail
            .as_deref()
            .expect("image should have detail");
        assert!(detail.contains("800"), "detail should mention width");
        assert!(detail.contains("600"), "detail should mention height");
    }

    #[tokio::test]
    async fn no_results_item_has_no_detail() {
        let manager = make_manager();
        manager.seed_history_for_test(vec![text_item("hello")]);

        let ext = ClipboardExtension::new(manager);
        // Query that won't match "hello"
        let results = ext.on_search("zzzzz").await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, "clipboard-no-results");
        assert!(results[0].detail.is_none());
    }

    #[tokio::test]
    async fn multiple_items_each_have_their_own_detail() {
        let manager = make_manager();
        manager.seed_history_for_test(vec![text_item("first entry"), text_item("second entry")]);

        let ext = ClipboardExtension::new(manager);
        let results = ext.on_search("").await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].detail.as_deref(), Some("first entry"));
        assert_eq!(results[1].detail.as_deref(), Some("second entry"));
    }

    // ── truncate_text ─────────────────────────────────────────────────────────

    #[test]
    fn truncate_text_short_string_unchanged() {
        assert_eq!(ClipboardExtension::truncate_text("hello", 10), "hello");
    }

    #[test]
    fn truncate_text_exactly_at_limit_unchanged() {
        assert_eq!(ClipboardExtension::truncate_text("hello", 5), "hello");
    }

    #[test]
    fn truncate_text_over_limit_appends_ellipsis() {
        assert_eq!(
            ClipboardExtension::truncate_text("hello world", 5),
            "hello..."
        );
    }

    #[test]
    fn truncate_text_empty_string() {
        assert_eq!(ClipboardExtension::truncate_text("", 10), "");
    }

    // ── format_timestamp ─────────────────────────────────────────────────────

    #[test]
    fn format_timestamp_under_one_minute_is_just_now() {
        let ts = chrono::Local::now() - chrono::TimeDelta::seconds(30);
        assert_eq!(ClipboardExtension::format_timestamp(&ts), "Just now");
    }

    #[test]
    fn format_timestamp_one_minute_singular() {
        let ts = chrono::Local::now() - chrono::TimeDelta::minutes(1);
        assert_eq!(ClipboardExtension::format_timestamp(&ts), "1 minute ago");
    }

    #[test]
    fn format_timestamp_multiple_minutes_plural() {
        let ts = chrono::Local::now() - chrono::TimeDelta::minutes(5);
        assert_eq!(ClipboardExtension::format_timestamp(&ts), "5 minutes ago");
    }

    #[test]
    fn format_timestamp_one_hour_singular() {
        let ts = chrono::Local::now() - chrono::TimeDelta::hours(1);
        assert_eq!(ClipboardExtension::format_timestamp(&ts), "1 hour ago");
    }

    #[test]
    fn format_timestamp_multiple_hours_plural() {
        let ts = chrono::Local::now() - chrono::TimeDelta::hours(3);
        assert_eq!(ClipboardExtension::format_timestamp(&ts), "3 hours ago");
    }

    #[test]
    fn format_timestamp_multiple_days_plural() {
        let ts = chrono::Local::now() - chrono::TimeDelta::days(2);
        assert_eq!(ClipboardExtension::format_timestamp(&ts), "2 days ago");
    }
}

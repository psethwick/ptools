use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

fn default_auto_load() -> bool {
    true
}

/// One option in a `PreferenceSpec` of type `"dropdown"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreferenceDropdownItem {
    pub title: String,
    pub value: String,
}

/// Declaration of a single user-configurable preference for a JS/TS extension.
/// Parsed from the `"preferences"` array in `extension.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreferenceSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub title: String,
    #[serde(default)]
    pub required: bool,
    /// Default value — string, bool, or null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Only for `"dropdown"` type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data: Vec<PreferenceDropdownItem>,
}

/// A single tag chip inside a `DetailMetadataRow::TagList`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagItem {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// One row in the structured metadata sidebar of a `List.Item.Detail` panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DetailMetadataRow {
    Label {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
    },
    Link {
        title: String,
        text: String,
        target: String,
    },
    Separator,
    TagList {
        title: String,
        #[serde(default)]
        tags: Vec<TagItem>,
    },
}

/// A single action in an `ActionPanel`, beyond the item's primary action.
/// Populated from `<ActionPanel>` / `<Action>` props in JS/TS extensions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtraAction {
    pub title: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// Human-readable shortcut label, e.g. "⌘C" or "⌃⇧N".
    #[serde(default)]
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionItem {
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<String>,
    pub action: String,
    pub id: Option<String>,
    /// Extended content shown in the Detail side panel. Plain text or simple markdown.
    #[serde(default)]
    pub detail: Option<String>,
    /// Right-side accessory labels rendered as small dimmed chips.
    /// Each entry is a short display string (from `accessories[n].text` or
    /// `accessories[n].tag.value` in the Raycast API).
    #[serde(default)]
    pub accessories: Vec<String>,
    /// Additional actions from `<ActionPanel>` children. The first entry is
    /// also the primary action; subsequent entries are shown in the panel and
    /// can be triggered via their shortcut keys.
    #[serde(default)]
    pub extra_actions: Vec<ExtraAction>,
    /// Structured metadata rows shown in the right column of the detail panel.
    /// Populated from `<List.Item.Detail.Metadata>` children in JS/TS extensions.
    #[serde(default, rename = "detailMetadata")]
    pub detail_metadata: Vec<DetailMetadataRow>,
    /// Raw RGBA pixels for thumbnail display. Not serialized — only set by built-in extensions.
    #[serde(skip)]
    pub thumbnail_rgba: Option<(usize, usize, Vec<u8>)>,
    /// Non-zero column count signals that this item belongs to a `Grid` view.
    /// The value is derived from the `Grid`'s `itemSize` prop:
    /// `small` → 5, `medium` (default) → 4, `large` → 3.
    /// `None` means the item belongs to a regular `List`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gridColumns"
    )]
    pub grid_columns: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMetadata {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub language: ExtensionLanguage,
    pub entry_point: String,
    pub permissions: Vec<String>,
    /// If false, this extension is excluded from the global broadcast search and
    /// only appears when explicitly activated (e.g. via a mode shortcut).
    #[serde(default = "default_auto_load")]
    pub auto_load: bool,
    /// Human-readable display title (e.g. from a Raycast `package.json` command
    /// `title` field). Falls back to `name` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// User-configurable preferences declared in `extension.json`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preferences: Vec<PreferenceSpec>,
    /// `true` when the extension was loaded from the dev `./extensions/` directory
    /// rather than the user's `~/.pterry/extensions/` directory.
    /// Set by the loader; never serialised into extension.json.
    #[serde(default, skip_serializing)]
    pub is_development: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExtensionLanguage {
    JavaScript,
    TypeScript,
    Rust,
}

#[derive(Debug)]
pub enum ExtensionError {
    RuntimeError(String),
    LoadError(String),
    ExecutionError(String),
    NotFound(String),
}

impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtensionError::RuntimeError(msg) => write!(f, "Runtime error: {msg}"),
            ExtensionError::LoadError(msg) => write!(f, "Load error: {msg}"),
            ExtensionError::ExecutionError(msg) => write!(f, "Execution error: {msg}"),
            ExtensionError::NotFound(msg) => write!(f, "Not found: {msg}"),
        }
    }
}

impl std::error::Error for ExtensionError {}

#[async_trait]
pub trait Extension: Send + Sync + Debug {
    /// Get extension metadata
    fn metadata(&self) -> &ExtensionMetadata;

    /// Initialize the extension (called once when loaded)
    async fn initialize(&mut self) -> Result<(), ExtensionError>;

    /// Handle search query
    async fn on_search(&self, query: &str) -> Result<Vec<ExtensionItem>, ExtensionError>;

    /// Handle action execution
    async fn on_action(&self, action: &str, item_id: Option<&str>) -> Result<(), ExtensionError>;

    /// Cleanup resources (called when extension is unloaded)
    async fn cleanup(&self) -> Result<(), ExtensionError>;

    /// Get the extension's unique identifier
    fn id(&self) -> String {
        format!("{}@{}", self.metadata().name, self.metadata().version)
    }

    /// Check if extension supports the given permission
    fn has_permission(&self, permission: &str) -> bool {
        self.metadata()
            .permissions
            .contains(&permission.to_string())
    }

    /// For non-auto-load extensions, return a single "portal" item that appears
    /// in the global search list. Selecting it triggers `enter-mode:<name>`.
    /// Returns `None` by default (auto-load extensions don't need this).
    fn launcher_item(&self) -> Option<ExtensionItem> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    // ── MockExtension ─────────────────────────────────────────────────────────

    /// Minimal concrete Extension used to exercise default trait methods.
    #[derive(Debug)]
    struct MockExtension {
        meta: ExtensionMetadata,
    }

    #[async_trait]
    impl Extension for MockExtension {
        fn metadata(&self) -> &ExtensionMetadata {
            &self.meta
        }
        async fn initialize(&mut self) -> Result<(), ExtensionError> {
            Ok(())
        }
        async fn on_search(&self, _: &str) -> Result<Vec<ExtensionItem>, ExtensionError> {
            Ok(vec![])
        }
        async fn on_action(&self, _: &str, _: Option<&str>) -> Result<(), ExtensionError> {
            Ok(())
        }
        async fn cleanup(&self) -> Result<(), ExtensionError> {
            Ok(())
        }
    }

    fn mock_ext(name: &str, version: &str, permissions: Vec<String>) -> MockExtension {
        MockExtension {
            meta: ExtensionMetadata {
                name: name.to_string(),
                version: version.to_string(),
                description: None,
                author: None,
                language: ExtensionLanguage::JavaScript,
                entry_point: format!("{name}.js"),
                permissions,
                auto_load: true,
                title: None,
                preferences: vec![],
                is_development: false,
            },
        }
    }

    /// `id()` returns `"<name>@<version>"`.
    #[test]
    fn id_format_is_name_at_version() {
        let ext = mock_ext("my-ext", "2.3.4", vec![]);
        assert_eq!(ext.id(), "my-ext@2.3.4");
    }

    /// `has_permission()` returns true for each listed permission.
    #[test]
    fn has_permission_returns_true_when_listed() {
        let ext = mock_ext(
            "perm-test",
            "1.0.0",
            vec!["clipboard".to_string(), "network".to_string()],
        );
        assert!(ext.has_permission("clipboard"));
        assert!(ext.has_permission("network"));
    }

    /// `has_permission()` returns false for permissions not in the list.
    #[test]
    fn has_permission_returns_false_when_absent() {
        let ext = mock_ext("perm-test", "1.0.0", vec!["clipboard".to_string()]);
        assert!(!ext.has_permission("network"));
        assert!(!ext.has_permission("filesystem"));
    }

    /// Default `launcher_item()` returns None (auto-load extensions don't override it).
    #[test]
    fn launcher_item_returns_none_by_default() {
        let ext = mock_ext("launcher-test", "1.0.0", vec![]);
        assert!(ext.launcher_item().is_none());
    }

    fn base_item_json(extra: &str) -> String {
        format!(r#"{{"title":"t","subtitle":null,"icon":null,"action":"a","id":null{extra}}}"#)
    }

    #[test]
    fn detail_defaults_to_none_when_field_is_absent() {
        // JSON from an older extension that doesn't include "detail"
        let json = base_item_json("");
        let item: ExtensionItem = serde_json::from_str(&json).unwrap();
        assert!(item.detail.is_none());
    }

    #[test]
    fn detail_is_none_when_explicitly_null() {
        let json = base_item_json(r#","detail":null"#);
        let item: ExtensionItem = serde_json::from_str(&json).unwrap();
        assert!(item.detail.is_none());
    }

    #[test]
    fn detail_round_trips_through_serde() {
        let json = base_item_json(r#","detail":"full clipboard content here""#);
        let item: ExtensionItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item.detail.as_deref(), Some("full clipboard content here"));

        let re_serialized = serde_json::to_string(&item).unwrap();
        let item2: ExtensionItem = serde_json::from_str(&re_serialized).unwrap();
        assert_eq!(item2.detail.as_deref(), Some("full clipboard content here"));
    }

    // ── DetailMetadataRow tests ───────────────────────────────────────────────

    #[test]
    fn detail_metadata_defaults_to_empty_when_absent() {
        let json = base_item_json("");
        let item: ExtensionItem = serde_json::from_str(&json).unwrap();
        assert!(item.detail_metadata.is_empty());
    }

    #[test]
    fn detail_metadata_label_round_trips() {
        let json =
            base_item_json(r#","detailMetadata":[{"type":"label","title":"Stars","text":"88"}]"#);
        let item: ExtensionItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item.detail_metadata.len(), 1);
        match &item.detail_metadata[0] {
            DetailMetadataRow::Label { title, text, .. } => {
                assert_eq!(title, "Stars");
                assert_eq!(text.as_deref(), Some("88"));
            }
            _ => panic!("expected Label"),
        }
    }

    #[test]
    fn detail_metadata_link_round_trips() {
        let json = base_item_json(
            r#","detailMetadata":[{"type":"link","title":"Repo","text":"raycast/extensions","target":"https://github.com/raycast/extensions"}]"#,
        );
        let item: ExtensionItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item.detail_metadata.len(), 1);
        match &item.detail_metadata[0] {
            DetailMetadataRow::Link {
                title,
                text,
                target,
            } => {
                assert_eq!(title, "Repo");
                assert_eq!(text, "raycast/extensions");
                assert_eq!(target, "https://github.com/raycast/extensions");
            }
            _ => panic!("expected Link"),
        }
    }

    #[test]
    fn detail_metadata_separator_round_trips() {
        let json = base_item_json(r#","detailMetadata":[{"type":"separator"}]"#);
        let item: ExtensionItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item.detail_metadata.len(), 1);
        assert!(matches!(
            item.detail_metadata[0],
            DetailMetadataRow::Separator
        ));
    }

    #[test]
    fn detail_metadata_taglist_round_trips() {
        let json = base_item_json(
            r##","detailMetadata":[{"type":"tagList","title":"Tags","tags":[{"text":"Rust"},{"text":"egui","color":"#FF6B35"}]}]"##,
        );
        let item: ExtensionItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item.detail_metadata.len(), 1);
        match &item.detail_metadata[0] {
            DetailMetadataRow::TagList { title, tags } => {
                assert_eq!(title, "Tags");
                assert_eq!(tags.len(), 2);
                assert_eq!(tags[0].text, "Rust");
                assert!(tags[0].color.is_none());
                assert_eq!(tags[1].text, "egui");
                assert_eq!(tags[1].color.as_deref(), Some("#FF6B35"));
            }
            _ => panic!("expected TagList"),
        }
    }

    #[test]
    fn detail_metadata_label_without_text_round_trips() {
        let json = base_item_json(r#","detailMetadata":[{"type":"label","title":"Status"}]"#);
        let item: ExtensionItem = serde_json::from_str(&json).unwrap();
        match &item.detail_metadata[0] {
            DetailMetadataRow::Label { title, text, .. } => {
                assert_eq!(title, "Status");
                assert!(text.is_none());
            }
            _ => panic!("expected Label"),
        }
    }

    #[test]
    fn thumbnail_rgba_is_not_serialized() {
        let item = ExtensionItem {
            title: "img".into(),
            subtitle: None,
            icon: None,
            action: "a".into(),
            id: None,
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: Some((2, 2, vec![0u8; 16])),
            grid_columns: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(!json.contains("thumbnail_rgba"));

        let deserialized: ExtensionItem = serde_json::from_str(&json).unwrap();
        assert!(deserialized.thumbnail_rgba.is_none());
    }
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

fn frequency_path() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".pterry").join("selection_frequency.json")
}

/// Tracks how many times each item action-string has been selected.
/// Persisted as JSON at `~/.pterry/selection_frequency.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SelectionFrequency {
    counts: HashMap<String, u32>,
}

impl SelectionFrequency {
    pub fn load() -> Self {
        let path = frequency_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = frequency_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())
    }

    /// Increment the selection count for `action_key`.
    pub fn increment(&mut self, action_key: &str) {
        *self.counts.entry(action_key.to_string()).or_insert(0) += 1;
    }

    /// Return the current selection count for `action_key` (0 if never selected).
    pub fn score(&self, action_key: &str) -> u32 {
        self.counts.get(action_key).copied().unwrap_or(0)
    }
}

/// Stable-sort `items` so higher-frequency items appear first.
/// Items with equal frequency preserve their original relative order.
pub fn boost_by_frequency(
    mut items: Vec<crate::extension_trait::ExtensionItem>,
    freq: &SelectionFrequency,
) -> Vec<crate::extension_trait::ExtensionItem> {
    items.sort_by(|a, b| {
        let sa = freq.score(&a.action);
        let sb = freq.score(&b.action);
        sb.cmp(&sa) // descending: higher score first
    });
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_trait::ExtensionItem;

    fn make_item(action: &str) -> ExtensionItem {
        ExtensionItem {
            title: action.to_string(),
            subtitle: None,
            icon: None,
            action: action.to_string(),
            id: None,
            detail: None,
            accessories: vec![],
            extra_actions: vec![],
            detail_metadata: vec![],
            thumbnail_rgba: None,
            grid_columns: None,
        }
    }

    #[test]
    fn score_returns_zero_for_unknown_action() {
        let freq = SelectionFrequency::default();
        assert_eq!(freq.score("launch-app:/usr/bin/firefox"), 0);
    }

    #[test]
    fn increment_increases_score_by_one() {
        let mut freq = SelectionFrequency::default();
        freq.increment("launch-app:/usr/bin/firefox");
        assert_eq!(freq.score("launch-app:/usr/bin/firefox"), 1);
    }

    #[test]
    fn multiple_increments_accumulate() {
        let mut freq = SelectionFrequency::default();
        for _ in 0..5 {
            freq.increment("open-url:https://example.com");
        }
        assert_eq!(freq.score("open-url:https://example.com"), 5);
    }

    #[test]
    fn different_actions_tracked_independently() {
        let mut freq = SelectionFrequency::default();
        freq.increment("action-a");
        freq.increment("action-a");
        freq.increment("action-b");
        assert_eq!(freq.score("action-a"), 2);
        assert_eq!(freq.score("action-b"), 1);
        assert_eq!(freq.score("action-c"), 0);
    }

    #[test]
    fn round_trip_serialization_preserves_counts() {
        let mut freq = SelectionFrequency::default();
        freq.increment("action-x");
        freq.increment("action-x");
        freq.increment("action-y");

        let json = serde_json::to_string(&freq).unwrap();
        let restored: SelectionFrequency = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.score("action-x"), 2);
        assert_eq!(restored.score("action-y"), 1);
        assert_eq!(restored.score("action-z"), 0);
    }

    #[test]
    fn boost_empty_list_returns_empty() {
        let freq = SelectionFrequency::default();
        let result = boost_by_frequency(vec![], &freq);
        assert!(result.is_empty());
    }

    #[test]
    fn boost_single_item_unchanged() {
        let freq = SelectionFrequency::default();
        let items = vec![make_item("a")];
        let result = boost_by_frequency(items, &freq);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].action, "a");
    }

    #[test]
    fn boost_zero_frequency_preserves_original_order() {
        let freq = SelectionFrequency::default();
        let items = vec![make_item("first"), make_item("second"), make_item("third")];
        let result = boost_by_frequency(items, &freq);
        assert_eq!(result[0].action, "first");
        assert_eq!(result[1].action, "second");
        assert_eq!(result[2].action, "third");
    }

    #[test]
    fn boost_moves_frequent_item_to_front() {
        let mut freq = SelectionFrequency::default();
        freq.increment("second");
        freq.increment("second");
        freq.increment("second");

        let items = vec![make_item("first"), make_item("second"), make_item("third")];
        let result = boost_by_frequency(items, &freq);
        assert_eq!(
            result[0].action, "second",
            "most-selected item should be first"
        );
        // first and third have same frequency; their relative order is preserved
        assert_eq!(result[1].action, "first");
        assert_eq!(result[2].action, "third");
    }

    #[test]
    fn boost_respects_frequency_ranking() {
        let mut freq = SelectionFrequency::default();
        freq.increment("c");
        freq.increment("a");
        freq.increment("a");
        freq.increment("b");
        // a=2, b=1, c=1

        let items = vec![make_item("a"), make_item("b"), make_item("c")];
        let result = boost_by_frequency(items, &freq);
        assert_eq!(result[0].action, "a", "highest count first");
        // b and c are tied at 1; original order preserved
        assert_eq!(result[1].action, "b");
        assert_eq!(result[2].action, "c");
    }

    #[test]
    fn boost_is_stable_for_tied_items() {
        // Items with equal frequency must preserve their original relative order.
        let mut freq = SelectionFrequency::default();
        freq.increment("b");
        freq.increment("d");
        // a=0, b=1, c=0, d=1 — b and d tied at 1, a and c tied at 0
        let items = vec![
            make_item("a"),
            make_item("b"),
            make_item("c"),
            make_item("d"),
        ];
        let result = boost_by_frequency(items, &freq);
        // b and d are first (both score 1); a and c are last (score 0)
        let top: Vec<&str> = result[..2].iter().map(|i| i.action.as_str()).collect();
        let bottom: Vec<&str> = result[2..].iter().map(|i| i.action.as_str()).collect();
        assert!(top.contains(&"b"));
        assert!(top.contains(&"d"));
        assert!(bottom.contains(&"a"));
        assert!(bottom.contains(&"c"));
    }
}

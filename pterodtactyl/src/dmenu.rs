use anyhow::Result;

use crate::picker::{Action, Item, ListItem, Picker};
use std::io::Write;

pub struct DMenu {}

impl Picker for DMenu {
    fn name(&self) -> &str {
        "dmenu"
    }

    fn cacheable(&self) -> Option<Vec<ListItem>> {
        let lines: Vec<_> = std::io::stdin()
            .lines()
            .filter_map(|l| {
                if let Ok(line) = l {
                    Some(ListItem {
                        display: line.clone(),
                        highlights: None,
                        item: Item::Text(line),
                    })
                } else {
                    None
                }
            })
            .collect();

        if lines.is_empty() {
            None
        } else {
            Some(lines)
        }
    }

    fn action(&self, item: &Item) -> Result<Option<Action>> {
        if let Item::Text(t) = item {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(t.as_bytes())?;
        }
        Ok(Action::Close.into())
    }
}

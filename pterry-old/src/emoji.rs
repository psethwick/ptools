use crate::picker::{Item, ListItem, Picker};

pub struct Emoji {}

// TODO: this is screwed up right now, I really need to show images instead I think
// https://github.com/adobe-fonts/noto-emoji-svg
// license seems ok, I just gotta implement probably
impl Picker for Emoji {
    fn name(&self) -> &str {
        "emoji"
    }

    fn cacheable(&self) -> Option<Vec<ListItem>> {
        Some(
            emojis::iter()
                .map(|e| ListItem {
                    display: format!("{} {}", e.as_str(), e.name()),
                    highlights: None,
                    item: Item::Text(e.as_str().to_owned()),
                })
                .collect::<Vec<_>>(),
        )
    }
}

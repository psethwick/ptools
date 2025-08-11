use std::{ops::Deref, sync::Arc};

use crate::{
    calculator::Calculator,
    desktop_launcher::DesktopLauncher,
    emoji::Emoji,
    language::Verb,
    notes::Notes,
    picker::{ListItem, Picker, VerbHandler},
};

pub struct Core {
    default_extensions: Vec<Arc<dyn Picker>>,
}

impl Picker for Core {
    fn name(&self) -> &str {
        ""
    }

    fn cacheable(&self) -> Option<Vec<ListItem>> {
        let subs: Vec<_> = self
            .default_extensions
            .iter()
            .filter_map(|p| p.cacheable())
            .flat_map(|x| x.deref().to_owned())
            .collect();
        Some(subs)
    }

    fn dynamic(&self, input: &str) -> Option<Vec<ListItem>> {
        let mut res = vec![];
        for e in &self.default_extensions {
            if let Some(d) = e.dynamic(input) {
                res.extend(d);
            }
        }
        if res.is_empty() { None } else { Some(res) }
    }

    fn verbs(&self) -> Option<Vec<(Verb, VerbHandler)>> {
        Some(
            self.default_extensions
                .iter()
                .filter_map(|e| e.verbs())
                .flatten()
                .collect(),
        )
    }

    fn sub_pickers(&self) -> Option<Vec<Arc<dyn Picker>>> {
        Some(vec![Arc::new(Emoji {})])
    }
}

impl Default for Core {
    fn default() -> Self {
        Self {
            default_extensions: vec![
                Arc::new(Notes {}),
                Arc::new(Calculator {}),
                Arc::<DesktopLauncher>::default(),
            ],
        }
    }
}

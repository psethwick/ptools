use crate::picker::{Action, Item, ListItem, Picker};
use anyhow::Result;

pub struct Todoist {
    items: Vec<ListItem>,
}

impl Todoist {
    pub fn new(items: Vec<ListItem>) -> Self {
        Self { items }
    }
}

impl Picker for Todoist {
    fn name(&self) -> &str {
        "todoist"
    }

    fn action(&self, item: &Item) -> Result<Option<Action>> {
        match item {
            // TODO: gonna need some kinda id
            // maybe this should take ListItem instead of Item
            Item::Checkable(checked, strng) => {
                dbg!(checked, strng);
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

// impl eframe::App for Todoist {
//     fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
//         if let Ok(new_output) = self.rx.try_recv() {
//             match new_output {
//                 ToMeatspace::Items(choices) => {
//                     self.items = choices;
//                 }
//                 ToMeatspace::Clear => todo!(),
//                 ToMeatspace::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
//             }
//         }
//         egui::CentralPanel::default().show(ctx, |ui| {
//             for line in self.items.iter_mut() {
//                 if let Item::Checkable(mut chk, strn) = &line.item {
//                     let res = ui.checkbox(&mut chk, strn);
//                     if res.changed() {
//                         let _ = self.tx.send(ToCore::Selected(line.item.clone()));
//                         line.item = Item::Checkable(chk, strn.to_owned());
//                     }
//                 }
//             }
//         });
//     }
// }

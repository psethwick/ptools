use crate::picker::{Item, ListItem, Picker};
use numbat::{pretty_print::PrettyPrint, Context, InterpreterResult::Value};

pub struct Calculator {}

impl Picker for Calculator {
    fn name(&self) -> &str {
        "calculator"
    }

    fn dynamic(&self, input: &str) -> Option<Vec<ListItem>> {
        if input.is_empty() {
            return None;
        }
        let mut ctx = Context::new_without_importer();
        // todo there's get completions for ..
        match ctx.interpret(input, numbat::resolver::CodeSource::Text) {
            Ok((stms, calc_res)) => {
                let mut display = String::new();
                let mut copy = String::new();
                for s in stms {
                    let markup = s.pretty_print();
                    display += &format!("{}", markup).to_owned();
                }
                if let Value(v) = calc_res {
                    copy += &format!("{}", v);
                    display += &format!(" = {}", copy);
                }
                vec![ListItem {
                    display,
                    highlights: None,
                    item: Item::Text(copy),
                }]
                .into()
            }
            Err(_) => None,
        }
    }
}

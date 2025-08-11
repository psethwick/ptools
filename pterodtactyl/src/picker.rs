use anyhow::Result;
use std::io::Write;
use std::sync::Arc;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use crate::language::Verb;

#[derive(Debug, Clone)]
pub struct Executable {
    pub exec: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum PickerOp {
    Push(String),
    Pop,
}

#[derive(Debug, Clone)]
pub enum Item {
    Executable(Executable),
    File(PathBuf),
    Text(String),
    Checkable(bool, String),
    PickerStackOp(PickerOp),
}

#[derive(Debug, Clone)]
pub struct ListItem {
    pub display: String, // TODO: allow some layout control from extension ... Element??
    pub highlights: Option<Vec<usize>>,
    pub item: Item,
}

pub enum Action {
    Close,
    Selected(Item),
}

pub enum ToCore {
    InputChanged(String),
    HandleVerb(String),
    Selected(Item),
    Exit,
}

pub enum Element {
    Image(PathBuf),
    Text(String),
    Checkbox(bool, String),
}

pub enum ToMeatspace {
    Items(Vec<ListItem>),
    Clear,
    Close,
}

pub type VerbHandler = Box<dyn Fn(&str) -> Result<Option<ToMeatspace>>>;

pub fn next_picker_by_name(picker: Arc<dyn Picker>, name: &str) -> Option<Arc<dyn Picker>> {
    picker
        .sub_pickers()
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.name() == name)
}

pub trait Picker {
    fn name(&self) -> &str;

    fn cacheable(&self) -> Option<Vec<ListItem>> {
        None
    }

    fn dynamic(&self, _: &str) -> Option<Vec<ListItem>> {
        None
    }

    // TODO: I think this lives elsewhere
    // some other trait
    fn verbs(&self) -> Option<Vec<(Verb, VerbHandler)>> {
        None
    }

    fn sub_pickers(&self) -> Option<Vec<Arc<dyn Picker>>> {
        None
    }

    fn action(&self, item: &Item) -> Result<Option<Action>> {
        match item {
            Item::Checkable(b, s) => {
                dbg!("YES", b, s);
                Ok(None)
            }
            Item::Executable(e) => {
                dbg!(item);
                let mut cmd = Command::new(&e.exec);

                // https://specifications.freedesktop.org/desktop-entry-spec/latest/ar01s07.html
                // this is in the firefox one:
                // %u A single URL. Local files may either be passed as file: URLs or as file path.
                // I may at some point care about 'Desktop Actions'?
                for arg in &e.args {
                    // ignore all %<c> directives
                    if !arg.contains('%') {
                        cmd.arg(arg);
                    }
                }
                let _ = cmd.spawn()?;
                Ok(Action::Close.into())
            }
            Item::Text(t) => {
                let text = t.clone();
                // TODO: platform specific
                let mut child = Command::new("xclip")
                    .arg("-selection")
                    .arg("clipboard")
                    .stdin(Stdio::piped())
                    .spawn()?;
                child
                    .stdin
                    .take()
                    .expect("Failed to open stdin")
                    .write_all(text.as_bytes())
                    .expect("Failed to write to stdin");
                Ok(Action::Close.into())
            }
            Item::File(path_buf) => {
                // TODO: platform specific
                let _ = Command::new("xdg-open").arg(path_buf).spawn()?;
                Ok(Action::Close.into())
            }
            Item::PickerStackOp(op) => Ok(Action::Selected(Item::PickerStackOp(op.clone())).into()),
        }
    }
}

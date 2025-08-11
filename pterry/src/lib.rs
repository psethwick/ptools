#![warn(clippy::all, rust_2018_idioms)]
// core
pub mod core;
pub mod language;
pub mod picker;
pub mod state;

// builtins
pub mod calculator;
pub mod desktop_launcher;
pub mod dmenu;
pub mod emoji;
pub mod notes;
pub mod todoist;

// gui
pub mod everything_box;

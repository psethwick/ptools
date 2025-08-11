#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use anyhow::Result;
use egui::X11WindowType;
use pterry::{
    core::Core,
    everything_box::EverythingBox,
    picker::{ToCore, ToMeatspace, next_picker_by_name},
    state::State,
};
use std::sync::{Arc, mpsc::channel};
use tokio::runtime::Runtime;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser, Clone)]
struct Cli {
    #[command(subcommand)]
    command: Option<Mode>,
}

#[derive(Debug, Subcommand, Clone)]
enum Mode {
    Picker { name: String },
}

fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    let args = Cli::parse();
    let rt = Runtime::new().expect("Unable to create Runtime");

    let _enter = rt.enter();

    let (itx, irx) = channel::<ToCore>();
    let (ctx, crx) = channel::<ToMeatspace>();
    let loopback = itx.clone();

    std::thread::spawn(move || {
        let mut state = State::default();

        #[allow(clippy::unwrap_or_default)]
        if let Some(cmd) = args.command {
            match cmd {
                Mode::Picker { name } => {
                    let new_picker = next_picker_by_name(state.current_picker.clone(), &name)
                        .unwrap_or(Arc::<Core>::default());
                    state.set_picker(new_picker);
                }
            }
        }

        rt.block_on(async move {
            state.event_loop(loopback, irx, ctx);
        })
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_max_inner_size([600.0, 400.0])
            .with_always_on_top()
            .with_transparent(true)
            .with_title_shown(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_window_type(X11WindowType::Dialog),
        centered: true,

        ..Default::default()
    };

    // TODO: this is probably its own tool?
    //  or maybe it isn't
    //  what should it do
    //  display data?
    //  plot?
    //  I sort of like the instant widget idea from <stuff>
    //  maybe it could pipe stuff back out over stdout?
    //
    // if let Some(Mode::View) = args2.command {
    //     let lines: Vec<_> = std::io::stdin()
    //         .lines()
    //         .filter_map(|l| {
    //             if let Ok(line) = l {
    //                 Some(ListItem {
    //                     display: line.clone(),
    //                     highlights: None,
    //                     item: Item::Checkable(false, line),
    //                 })
    //             } else {
    //                 None
    //             }
    //         })
    //         .collect();
    //     eframe::run_native(
    //         "Pterry",
    //         options,
    //         Box::new(|cc| {
    //             egui_extras::install_image_loaders(&cc.egui_ctx);
    //             Box::<View>::new(View::new("wat".into(), lines, itx, crx))
    //         }),
    //     )
    // } else {
    eframe::run_native(
        "Pterry",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            if cc.egui_ctx.system_theme().is_none() {
                cc.egui_ctx.set_theme(egui::Theme::Light);
            }
            Ok(Box::<EverythingBox>::new(EverythingBox::new(cc, itx, crx)))
        }),
    )
    // }
}

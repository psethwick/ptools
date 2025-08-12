#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
use eframe::egui;
use psync::source::Source;
use sqlx::{Pool, Sqlite, SqlitePool, migrate};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc::channel},
};
use tokio::runtime::Runtime;

fn get_data_dir() -> anyhow::Result<PathBuf> {
    let data_dir =
        dirs::data_dir().ok_or_else(|| anyhow::anyhow!("Failed to get data directory"))?;
    Ok(data_dir.join("psync"))
}

fn main() -> eframe::Result {
    env_logger::init();

    let rt = Runtime::new().expect("Failed to create Tokio runtime");
    let _enter = rt.enter();

    let pool = rt
        .block_on(async {
            let data_dir = get_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("psync.db");
            let pool =
                SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.to_str().unwrap()))
                    .await?;
            migrate!("./migrations").run(&pool).await?;
            Ok::<_, anyhow::Error>(pool)
        })
        .expect("Database initialization failed");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };
    eframe::run_native(
        "My egui App",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            if cc.egui_ctx.system_theme().is_none() {
                cc.egui_ctx.set_theme(egui::Theme::Light);
            }
            let app = MyApp {
                // pool,
                names: vec![],
                sources: vec![],
            };

            Ok(Box::new(app))
        }),
    )
}

struct MyApp {
    names: Vec<String>,
    sources: Vec<Source>,
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("My egui Application");
            ui.horizontal(|ui| {
                let name_label = ui.label("Your name: ");
                // ui.text_edit_singleline(&mut self.name)
                //     .labelled_by(name_label.id);
            });
            // ui.add(egui::Slider::new(&mut self.age, 0..=120).text("age"));
            // if ui.button("Increment").clicked() {
            //     self.age += 1;
            // }
            // ui.label(format!("Hello '{}', age {}", self.name, self.age));
        });
    }
}

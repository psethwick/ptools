use crate::picker::{Item, ListItem, PickerOp, ToCore, ToMeatspace};
use eframe::egui;
use egui::{text::LayoutJob, Color32, FontFamily, FontId, TextEdit, TextFormat, TextStyle};
use std::sync::mpsc::{Receiver, Sender};

/// UI elements to expose
/// label
/// checkbox
///
/// future
/// image?
/// big text box?
/// graph?
/// live updated view?
pub struct EverythingBox {
    startup: bool,
    input: String,
    index: usize,
    choices: Vec<ListItem>,
    tx: Sender<ToCore>,
    rx: Receiver<ToMeatspace>,
}

fn configure_text_styles(ctx: &egui::Context) {
    use FontFamily::{Monospace, Proportional};

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(25.0, Proportional)),
        (TextStyle::Body, FontId::new(16.0, Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, Monospace)),
        (TextStyle::Button, FontId::new(12.0, Proportional)),
        (TextStyle::Small, FontId::new(8.0, Proportional)),
    ]
    .into();
    ctx.set_style(style);
}

impl EverythingBox {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        tx: Sender<ToCore>,
        rx: Receiver<ToMeatspace>,
    ) -> Self {
        configure_text_styles(&cc.egui_ctx);
        Self {
            startup: true,
            tx,
            index: 0,
            rx,
            choices: Vec::new(),
            input: "".to_owned(),
        }
    }
}

impl eframe::App for EverythingBox {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.startup {
            if raw_input
                .events
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            pressed: _,
                            key: _,
                            physical_key: _,
                            repeat: _,
                            modifiers: _,
                        }
                    )
                })
                .count()
                > 0
            {
                self.startup = false;
            } else {
                raw_input.events = vec![];
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(new_output) = self.rx.try_recv() {
            match new_output {
                ToMeatspace::Items(choices) => {
                    self.choices = choices;
                }
                ToMeatspace::Clear => self.input = "".to_string(),
                ToMeatspace::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                let input_res = ui.add(TextEdit::singleline(&mut self.input));
                input_res.request_focus();
                if input_res.changed() {
                    let _ = self.tx.send(ToCore::InputChanged(self.input.clone()));
                    self.index = 0;
                }
                let len = self.choices.len();

                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if len > 0 {
                        let _ = self
                            .tx
                            .send(ToCore::Selected(self.choices[self.index].item.clone()));
                    } else {
                        let _ = self.tx.send(ToCore::HandleVerb(self.input.to_owned()));
                    }
                }

                if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    if self.index > 0 {
                        self.index -= 1;
                    } else {
                        self.index = len - 1;
                    }
                }

                if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    if self.index < len - 1 {
                        self.index += 1;
                    } else {
                        self.index = 0;
                    }
                }

                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    let _ = self
                        .tx
                        .send(ToCore::Selected(Item::PickerStackOp(PickerOp::Pop)));
                }

                for (i, c) in self.choices.iter().enumerate().skip(self.index) {
                    let row = match &c.highlights {
                        None => ui.label(&c.display),
                        Some(hl) => {
                            let mut layout_job = LayoutJob::default();
                            let normal = TextFormat::default();
                            let highlight_format = TextFormat {
                                underline: egui::Stroke {
                                    width: 0.5,
                                    color: Color32::from_rgb(0, 0, 0),
                                },
                                ..Default::default()
                            };
                            for (i, c) in c.display.chars().enumerate() {
                                if hl.contains(&i) {
                                    layout_job.append(
                                        &c.to_string(),
                                        0.0,
                                        highlight_format.clone(),
                                    );
                                } else {
                                    layout_job.append(&c.to_string(), 0.0, normal.clone());
                                }
                            }
                            ui.label(layout_job)
                        }
                    };
                    if i == self.index {
                        row.highlight();
                    }
                }
            });
        });
    }
}

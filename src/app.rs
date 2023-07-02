use crate::engine::Engine;
use crate::stave::Stave;
use crate::track::Lane;
use eframe::egui::Color32;
use eframe::{self, egui};
use egui_extras::{Size, StripBuilder};
use std::sync::{Arc, Mutex};

pub struct EmApp {
    engine: Arc<Mutex<Engine>>,
    stave: Stave,
}

impl PartialEq for EmApp {
    fn eq(&self, other: &Self) -> bool {
        self.stave == other.stave
    }
}

impl EmApp {
    pub fn new(engine: Arc<Mutex<Engine>>, track: Arc<Box<Lane>>) -> EmApp {
        EmApp {
            engine,
            stave: Stave {
                track,
                time_scale: 5e-9f32,
                cursor_position: 0,
            },
        }
    }
}

impl eframe::App for EmApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        ctx.set_pixels_per_point(1.5);
        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Space)) {
                // TODO Should handle failed lock gracefully.
                self.engine
                    .lock()
                    .expect("Lock engine for play/pause")
                    .toggle_pause();
            }
            ui.heading(format!("Emmate at {} px/pt", ui.ctx().pixels_per_point()));
            StripBuilder::new(ui)
                .size(Size::remainder())
                .size(Size::exact(25.0))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        self.stave.view(ui);
                    });
                    strip.cell(|mut ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Zoom in").clicked() {
                                self.stave.time_scale *= 1.05;
                            }
                            if ui.button("Zoom out").clicked() {
                                self.stave.time_scale /= 1.05;
                            }
                        });
                    })
                })
        });
    }
}

use std::sync::{Arc, Mutex, RwLock};

use eframe::egui::Response;
use eframe::{self, egui, CreationContext};
use egui_extras::{Size, StripBuilder};

use crate::engine::{Engine, StatusEvent};
use crate::stave::Stave;
use crate::track::Lane;

pub struct EmApp {
    engine: Arc<Mutex<Engine>>,
    stave: Arc<RwLock<Stave>>,
}

impl PartialEq for EmApp {
    fn eq(&self, other: &Self) -> bool {
        self.stave.read().unwrap().eq(&other.stave.read().unwrap())
    }
}

impl EmApp {
    pub fn new(ctx: &CreationContext, engine: Arc<Mutex<Engine>>, track: Arc<Box<Lane>>) -> EmApp {
        let app = EmApp {
            engine,
            stave: Arc::new(RwLock::new(Stave {
                track,
                time_scale: 5e-9f32,
                cursor_position: 0,
            })),
        };

        let engine_receiver_ctx = ctx.egui_ctx.clone();
        let stave2 = app.stave.clone();
        app.engine
            .lock()
            .unwrap()
            .set_status_receiver(Some(Box::new(move |ev| {
                // TODO Throttle updates (30..60 times per second should be enough).
                match ev {
                    StatusEvent::TransportTime(t) => {
                        if let Ok(mut locked) = stave2.try_write() {
                            locked.cursor_position = t
                        }
                    }
                }
                engine_receiver_ctx.request_repaint();
            })));
        app
    }
}

impl eframe::App for EmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                    if let Ok(mut stave) = self.stave.try_write() {
                        strip.cell(|ui| {
                            stave.view(ui);
                        });
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                if ui.button("Zoom in").clicked() {
                                    stave.time_scale *= 1.05;
                                }
                                if ui.button("Zoom out").clicked() {
                                    stave.time_scale /= 1.05;
                                }
                            });
                        })
                    }
                })
        });
    }
}

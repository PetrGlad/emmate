use std::sync::{mpsc, Arc, Mutex, RwLock};

use eframe::egui::Vec2;
use eframe::{self, egui, CreationContext};
use egui_extras::{Size, StripBuilder};

use crate::engine::{Engine, StatusEvent, TransportTime};
use crate::stave::Stave;
use crate::track::Lane;

enum Message {
    UpdateTransportTime(TransportTime),
}

pub struct EmApp {
    engine: Arc<Mutex<Engine>>,
    stave: Arc<RwLock<Stave>>,
    message_receiver: mpsc::Receiver<Message>,

    follow_playback: bool,
}

impl PartialEq for EmApp {
    fn eq(&self, other: &Self) -> bool {
        self.stave.read().unwrap().eq(&other.stave.read().unwrap())
    }
}

impl EmApp {
    pub fn new(ctx: &CreationContext, engine: Arc<Mutex<Engine>>, track: Arc<Box<Lane>>) -> EmApp {
        let (message_sender, message_receiver) = mpsc::channel();
        let app = EmApp {
            engine,
            stave: Arc::new(RwLock::new(Stave {
                track,
                time_scale: 2e-6f32,
                viewport_left: 0,
                cursor_position: 0,
            })),
            message_receiver,
            follow_playback: false,
        };

        let engine_receiver_ctx = ctx.egui_ctx.clone();
        app.engine
            .lock()
            .unwrap()
            .set_status_receiver(Some(Box::new(move |ev| {
                // TODO (optimization?) Throttle updates (30..60 times per second should be enough).
                //      Should not miss one-off updates, maybe skip only in same-event-type runs.
                match ev {
                    StatusEvent::TransportTime(t) => {
                        match message_sender.send(Message::UpdateTransportTime(t)) {
                            Ok(_) => engine_receiver_ctx.request_repaint(),
                            _ => (), // Will try next time.
                        }
                    }
                }
            })));
        app
    }
}

impl eframe::App for EmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(message) = self.message_receiver.try_iter().last() {
            match message {
                Message::UpdateTransportTime(t) => {
                    if let Ok(mut locked) = self.stave.try_write() {
                        locked.cursor_position = t;
                        if self.follow_playback {
                            let at = locked.cursor_position;
                            locked.scroll_to(at);
                        }
                    }
                }
            }
        }

        ctx.set_pixels_per_point(1.5);
        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Space)) {
                self.engine
                    .lock()
                    .expect("TODO Locking engine for play/pause should not fail (use a channel instead?).")
                    .toggle_pause();
            }
            ui.heading(format!("Emmate at {} px/pt", ui.ctx().pixels_per_point()));
            StripBuilder::new(ui)
                .size(Size::remainder())
                .size(Size::exact(25.0))
                .vertical(|mut strip| {
                    if let Ok(mut stave) = self.stave.try_write() {
                        strip.cell(|ui| {
                            let response = stave.view(ui);
                            // TODO Stave is writable here. Move this zoom logic into that function.
                            if let Some(hover_pos) = response.hover_pos() {
                                let zoom_factor = ui.input(|i| i.zoom_delta());
                                if zoom_factor != 1.0 {
                                    println!("[zoom] {:?}", zoom_factor);
                                    stave.zoom(zoom_factor, hover_pos.x - response.rect.min.x);
                                }
                                let scroll_delta = ui.input(|i| i.scroll_delta);
                                if scroll_delta != Vec2::ZERO {
                                    println!("[scroll] {:?}", scroll_delta);
                                    stave.scroll_by(scroll_delta.x);
                                }
                            }
                        });
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                if ui.button("Zoom in").clicked() {
                                    stave.time_scale *= 1.05;
                                }
                                if ui.button("Zoom out").clicked() {
                                    stave.time_scale /= 1.05;
                                }
                                if ui.button("< Shift <").clicked() {
                                    if let Some(x) = stave.viewport_left.checked_sub(1_000_000) {
                                        stave.viewport_left = x
                                    }
                                }
                                if ui.button("> Shift >").clicked() {
                                    if let Some(x) = stave.viewport_left.checked_add(1_000_000) {
                                        stave.viewport_left = x
                                    }
                                }
                                ui.checkbox(&mut self.follow_playback, "Follow playback")
                            });
                        })
                    }
                })
        });
    }
}

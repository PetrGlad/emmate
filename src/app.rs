use std::sync::mpsc;

use eframe::egui::Vec2;
use eframe::{self, egui, CreationContext};
use egui_extras::{Size, StripBuilder};

use crate::engine::{Engine, EngineCommand, StatusEvent, TransportTime};
use crate::stave::{Stave, StaveTime};
use crate::track_history::TrackHistory;

enum Message {
    UpdateTransportTime(TransportTime),
}

pub struct EmApp {
    stave: Stave,
    engine_command_send: mpsc::Sender<Box<EngineCommand>>,
    message_receiver: mpsc::Receiver<Message>,
    follow_playback: bool,
}

// impl PartialEq for EmApp {
//     fn eq(&self, other: &Self) -> bool {
//         self.stave.read().unwrap().eq(&other.stave.read().unwrap())
//     }
// }

impl EmApp {
    pub fn new(
        ctx: &CreationContext,
        engine_command_send: mpsc::Sender<Box<EngineCommand>>,
        history: TrackHistory,
    ) -> EmApp {
        let (message_sender, message_receiver) = mpsc::channel();
        let app = EmApp {
            stave: Stave::new(history),
            engine_command_send,
            message_receiver,
            follow_playback: false,
        };

        let engine_receiver_ctx = ctx.egui_ctx.clone();
        let engine_status_receiver = Box::new(move |ev| {
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
        });
        app.engine_command_send
            .send(Box::new(|engine| {
                engine.set_status_receiver(Some(engine_status_receiver));
            }))
            .unwrap();
        app
    }

    fn toggle_pause(&mut self) {
        self.engine_command_send
            .send(Box::new(|engine| engine.toggle_pause()))
            .unwrap();
    }
}

impl eframe::App for EmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(message) = self.message_receiver.try_iter().last() {
            match message {
                Message::UpdateTransportTime(t) => {
                    self.stave.cursor_position = t as StaveTime;
                    if self.follow_playback {
                        let at = self.stave.cursor_position;
                        self.stave.scroll_to(at as StaveTime);
                    }
                }
            }
        }

        ctx.set_pixels_per_point(1.5);
        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Space)) {
                self.toggle_pause();
            }
            ui.heading(format!(
                "🌲 {:} [{:} / {:}]",
                self.stave.history.directory.display(),
                self.stave.history.version(),
                self.stave.track_version.to_string()
            ));
            StripBuilder::new(ui)
                .size(Size::remainder())
                .size(Size::exact(20.0))
                .size(Size::exact(20.0))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        let response = self.stave.show(ui);
                        if let Some(hover_pos) = response.hover_pos() {
                            let zoom_factor = ui.input(|i| i.zoom_delta());
                            if zoom_factor != 1.0 {
                                self.stave.zoom(zoom_factor, hover_pos.x);
                            }
                            let scroll_delta = ui.input(|i| i.scroll_delta);
                            if scroll_delta != Vec2::ZERO {
                                self.stave.scroll_by(scroll_delta.x);
                            }
                            if response.middle_clicked() {
                                let at = self.stave.time_from_x(hover_pos.x);
                                self.stave.cursor_position = at; // Should be a Stave method?
                                self.engine_command_send
                                    .send(Box::new(move |engine| engine.seek(at as TransportTime)))
                                    .unwrap();
                            }
                        }
                    });
                    strip.cell(|ui| {
                        ui.horizontal(|ui| {
                            let left = ui.painter().clip_rect().min.x;
                            if ui.button("Zoom in").clicked() {
                                self.stave.zoom(1.05, left);
                            }
                            if ui.button("Zoom out").clicked() {
                                self.stave.zoom(1.0 / 1.05, left);
                            }
                            let scroll_step = ui.painter().clip_rect().size().x * 0.15;
                            if ui.button("< Scroll <").clicked() {
                                self.stave.scroll_by(-scroll_step);
                            }
                            if ui.button("> Scroll >").clicked() {
                                self.stave.scroll_by(scroll_step);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.follow_playback, "Follow playback");
                            if ui.button("⏮ Rewind").clicked() {
                                self.engine_command_send
                                    .send(Box::new(|engine| engine.seek(0)))
                                    .unwrap();
                            }
                            if ui.button("🔇 Stop it").clicked() {
                                self.engine_command_send
                                    .send(Box::new(Engine::reset))
                                    .unwrap();
                            }
                            if ui.button("🚩Save").clicked() {
                                // TODO Should not save when there are no changes.
                                self.stave.history.change_version(1);
                                self.stave
                                    .save_to(&self.stave.history.current_snapshot_path());
                            }
                            if ui.button("⤵ Undo").clicked() {
                                self.stave.history.change_version(-1);
                                self.stave
                                    .load_from(&self.stave.history.current_snapshot_path());
                            }
                            if ui.button("⤴ Redo").clicked() {
                                self.stave.history.change_version(1);
                                self.stave
                                    .load_from(&self.stave.history.current_snapshot_path());
                            }
                        });
                    })
                });
        });
    }
}

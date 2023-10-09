use std::sync::{mpsc, Arc, RwLock};

use eframe::egui::Vec2;
use eframe::{self, egui, CreationContext};
use egui_extras::{Size, StripBuilder};

use crate::engine::{Engine, EngineCommand, StatusEvent, TransportTime};
use crate::lane::Lane;
use crate::project::Project;
use crate::stave::{Stave, StaveTime};

enum Message {
    UpdateTransportTime(TransportTime),
}

pub struct EmApp {
    stave: Arc<RwLock<Stave>>,
    engine_command_send: mpsc::Sender<Box<EngineCommand>>,
    message_receiver: mpsc::Receiver<Message>,
    follow_playback: bool,
    project: Project,
}

impl PartialEq for EmApp {
    fn eq(&self, other: &Self) -> bool {
        self.stave.read().unwrap().eq(&other.stave.read().unwrap())
    }
}

impl EmApp {
    pub fn new(
        ctx: &CreationContext,
        engine_command_send: mpsc::Sender<Box<EngineCommand>>,
        track: Arc<RwLock<Lane>>,
        project: Project,
    ) -> EmApp {
        let (message_sender, message_receiver) = mpsc::channel();
        let app = EmApp {
            stave: Arc::new(RwLock::new(Stave::new(track))),
            engine_command_send,
            message_receiver,
            follow_playback: false,
            project,
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
                    if let Ok(mut locked) = self.stave.try_write() {
                        locked.cursor_position = t as StaveTime;
                        if self.follow_playback {
                            let at = locked.cursor_position;
                            locked.scroll_to(at as StaveTime);
                        }
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
                "🌳 {:} [{:}]",
                self.project.directory.display(),
                self.project.version()
            ));
            StripBuilder::new(ui)
                .size(Size::remainder())
                .size(Size::exact(20.0))
                .size(Size::exact(20.0))
                .vertical(|mut strip| {
                    if let Ok(mut stave) = self.stave.try_write() {
                        strip.cell(|ui| {
                            let response = stave.view(ui);
                            if let Some(hover_pos) = response.hover_pos() {
                                let zoom_factor = ui.input(|i| i.zoom_delta());
                                if zoom_factor != 1.0 {
                                    stave.zoom(zoom_factor, hover_pos.x);
                                }
                                let scroll_delta = ui.input(|i| i.scroll_delta);
                                if scroll_delta != Vec2::ZERO {
                                    stave.scroll_by(scroll_delta.x);
                                }
                                if response.middle_clicked() {
                                    let at = stave.time_from_x(hover_pos.x);
                                    stave.cursor_position = at; // Should be a Stave method?
                                    self.engine_command_send
                                        .send(Box::new(move |engine| {
                                            engine.seek(at as TransportTime)
                                        }))
                                        .unwrap();
                                }
                            }
                        });
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                let left = ui.painter().clip_rect().min.x;
                                if ui.button("Zoom in").clicked() {
                                    stave.zoom(1.05, left);
                                }
                                if ui.button("Zoom out").clicked() {
                                    stave.zoom(1.0 / 1.05, left);
                                }
                                let scroll_step = ui.painter().clip_rect().size().x * 0.15;
                                if ui.button("< Scroll <").clicked() {
                                    stave.scroll_by(-scroll_step);
                                }
                                if ui.button("> Scroll >").clicked() {
                                    stave.scroll_by(scroll_step);
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
                                    self.project.change_version(1);
                                    stave.save_to(&self.project.current_snapshot_path());
                                }
                                if ui.button("⤵ Undo").clicked() {
                                    self.project.change_version(-1);
                                    if !stave.load_from(&self.project.current_snapshot_path()) {
                                        self.project.change_version(1);
                                    }
                                }
                                if ui.button("⤴ Redo").clicked() {
                                    self.project.change_version(1);
                                    if !stave.load_from(&self.project.current_snapshot_path()) {
                                        self.project.change_version(-1);
                                    }
                                }
                            });
                        })
                    } else {
                        /* Source data is shared between UI and the engine. So either the
                        locking is needed or engine should get it's updated version without
                        contentions but maybe after a delay. If we keep using locking maybe
                        an some kind of an agent is needed to make updates without risking
                        RwLock contentions.
                        Also all editing commands should go to the stave, (that logic should
                        not be in the App) */
                        println!("Stave is busy. Skipping frame.");
                    }
                });
        });
    }
}

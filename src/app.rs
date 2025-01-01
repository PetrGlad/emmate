use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use eframe::egui::{Modifiers, Vec2};
use eframe::{self, egui, CreationContext};
use egui_extras::{Size, StripBuilder};

use crate::common::Time;
use crate::engine::{Engine, EngineCommand, StatusEvent};
use crate::project::Project;
use crate::stave::Stave;

enum Message {
    UpdateTime(Time),
}

pub struct EmApp {
    title: String,
    home_path: PathBuf,
    stave: Stave,
    engine_command_send: mpsc::Sender<Box<EngineCommand>>,
    message_receiver: mpsc::Receiver<Message>,
    follow_playback: bool,
}

impl EmApp {
    pub fn new(
        ctx: &CreationContext,
        engine_command_send: mpsc::Sender<Box<EngineCommand>>,
        project: Project,
    ) -> EmApp {
        let (message_sender, message_receiver) = mpsc::channel();

        let app = EmApp {
            title: project.title,
            home_path: project.home_path,
            stave: Stave::new(project.history),
            engine_command_send,
            message_receiver,
            follow_playback: false,
        };

        let engine_receiver_ctx = ctx.egui_ctx.clone();
        let engine_status_receiver = Box::new(move |ev| {
            match ev {
                StatusEvent::Time(t) => {
                    match message_sender.send(Message::UpdateTime(t)) {
                        Ok(_) => {
                            engine_receiver_ctx.request_repaint_after(Duration::from_micros(20_000))
                        }
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

    fn export(&mut self) {
        let mut path = self.home_path.clone();
        path.push("export");
        if !path.is_dir() {
            log::debug!("Creating export directory {}", path.to_string_lossy());
            fs::create_dir_all(&path).expect("Create export directory.");
        }
        path.push(
            chrono::Local::now()
                .format("%Y-%m-%d_%H-%M-%S.mid")
                .to_string(),
        );
        log::info!("Saving to {}", path.to_string_lossy());
        self.stave.save_to(&PathBuf::from(path));
    }

    fn engine_seek(&self, to: Time) {
        self.engine_command_send
            .send(Box::new(move |engine| engine.seek(to)))
            .unwrap();
    }
}

impl eframe::App for EmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(message) = self.message_receiver.try_iter().last() {
            match message {
                Message::UpdateTime(t) => {
                    self.stave.cursor_position = t;
                    if self.follow_playback {
                        let at = self.stave.cursor_position;
                        self.stave.scroll_to(at, 0.1);
                    }
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    Modifiers::NONE,
                    egui::Key::Space,
                ))
            }) {
                self.toggle_pause();
            } else if ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::CTRL, egui::Key::S))
            }) {
                self.export();
            } else if ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::F))
            }) {
                self.follow_playback = !self.follow_playback;
            } else if ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    Modifiers::NONE,
                    egui::Key::PageUp,
                ))
            }) {
                self.stave.scroll_by(ctx.available_rect().width() / -4.0);
            } else if ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    Modifiers::NONE,
                    egui::Key::PageDown,
                ))
            }) {
                self.stave.scroll_by(ctx.available_rect().width() / 4.0);
            }

            {
                let h = self.stave.history.borrow();
                ui.heading(format!("🌲 {} [{}]", self.title, h.version()));
            }
            StripBuilder::new(ui)
                .size(Size::remainder())
                .size(Size::exact(20.0))
                .size(Size::exact(20.0))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        let response = self.stave.show(ui);

                        if let Some(hover_pos) = response.ui_response.hover_pos() {
                            let dz = ui.input(|i| i.zoom_delta());
                            if dz != 1.0 {
                                self.stave.zoom(dz, hover_pos.x);
                            }
                            let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
                            if scroll_delta != Vec2::ZERO {
                                self.stave.scroll_by(scroll_delta.x);
                            }
                        }
                        if let Some(pos) = response.new_cursor_position {
                            self.engine_seek(pos);
                        }
                    });
                    strip.cell(|ui| {
                        ui.horizontal(|ui| {
                            let mouse_x = ui.painter().clip_rect().min.x;
                            if ui.button(" + ").clicked() {
                                self.stave.zoom(1.05, mouse_x);
                            }
                            if ui.button(" - ").clicked() {
                                self.stave.zoom(1.0 / 1.05, mouse_x);
                            }
                            let scroll_step = ui.painter().clip_rect().size().x * 0.15;
                            if ui.button(" << ").clicked() {
                                self.stave.scroll_by(-scroll_step);
                            }
                            if ui.button(" >> ").clicked() {
                                self.stave.scroll_by(scroll_step);
                            }
                            ui.checkbox(&mut self.follow_playback, "Follow playback");
                            if ui.button(" ⏮ ").clicked() {
                                self.engine_seek(0);
                            }
                            if ui.button("🔇").clicked() {
                                self.engine_command_send
                                    .send(Box::new(Engine::reset))
                                    .unwrap();
                            }
                            if ui.button("🚩Export").clicked() {
                                self.export();
                            }
                            if ui.button("⤵ Undo").clicked() {
                                self.stave.history.borrow_mut().undo(&mut vec![]);
                            }
                            if ui.button("⤴ Redo").clicked() {
                                self.stave.history.borrow_mut().redo(&mut vec![]);
                            }
                        });
                        ui.horizontal(|ui| {
                            // Status line
                            ui.label(format!(
                                "track_len={}  n_sel={}  t_sel={}  at={}s ",
                                self.stave.history.borrow().with_track(|t| t.events.len()),
                                self.stave.note_selection.count(),
                                self.stave.time_selection().as_ref().map_or(
                                    "()".to_string(),
                                    |sel| {
                                        format!(
                                            "[{}s,{}s)/{}s",
                                            Duration::from_micros(sel.0 as u64).as_secs(),
                                            Duration::from_micros(sel.1 as u64).as_secs(),
                                            Duration::from_micros((sel.1 - sel.0).abs() as u64)
                                                .as_secs()
                                        )
                                    },
                                ),
                                Duration::from_micros(self.stave.cursor_position as u64).as_secs()
                            ));
                        });
                    })
                });
        });
    }
}

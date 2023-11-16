use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use crate::common::Time;
use eframe::egui::{Modifiers, Vec2};
use eframe::{self, egui, CreationContext};
use egui_extras::{Size, StripBuilder};

use crate::engine::{Engine, EngineCommand, StatusEvent};
use crate::project::Project;
use crate::stave::{Bookmarks, Stave};

enum Message {
    UpdateTime(Time),
}

pub struct EmApp {
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

        let mut bookmarks_path = project.home_path.clone();
        bookmarks_path.push("bookmarks.mpack");
        let mut bookmarks = Bookmarks::new(&bookmarks_path);
        if bookmarks_path.is_file() {
            bookmarks.load_from(&bookmarks_path);
        }

        let app = EmApp {
            home_path: project.home_path,
            stave: Stave::new(project.history, bookmarks),
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
            println!("Creating {}", path.to_string_lossy());
            fs::create_dir_all(&path).expect("Create export directory.");
        }
        path.push(
            chrono::Local::now()
                .format("%Y-%m-%d_%H-%M-%S.mid")
                .to_string(),
        );
        println!("Saving to {}", path.to_string_lossy());
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
        self.stave.history.do_pending();
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

        ctx.set_pixels_per_point(1.5);
        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.input(|i| i.key_pressed(egui::Key::Space)) {
                self.toggle_pause();
            }
            ui.heading(format!(
                "🌲 {} [{} / {}]",
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
                        if let Some(hover_pos) = response.ui_response.hover_pos() {
                            let zoom_factor = ui.input(|i| i.zoom_delta());
                            if zoom_factor != 1.0 {
                                self.stave.zoom(zoom_factor, hover_pos.x);
                            }
                            let scroll_delta = ui.input(|i| i.scroll_delta);
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
                            if ui.button("Zoom in").clicked() {
                                self.stave.zoom(1.05, mouse_x);
                            }
                            if ui.button("Zoom out").clicked() {
                                self.stave.zoom(1.0 / 1.05, mouse_x);
                            }
                            let scroll_step = ui.painter().clip_rect().size().x * 0.15;
                            if ui.button("< Scroll").clicked() {
                                self.stave.scroll_by(-scroll_step);
                            }
                            if ui.button("Scroll >").clicked() {
                                self.stave.scroll_by(scroll_step);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.follow_playback, "Follow playback");
                            if ui.button("⏮ Rewind").clicked() {
                                self.engine_seek(0);
                            }
                            if ui.button("🔇 Stop it").clicked() {
                                self.engine_command_send
                                    .send(Box::new(Engine::reset))
                                    .unwrap();
                            }
                            if ui.input_mut(|i| {
                                i.consume_shortcut(&egui::KeyboardShortcut::new(
                                    Modifiers::CTRL,
                                    egui::Key::S,
                                ))
                            }) {
                                self.export();
                            }
                            if ui.button("🚩Export").clicked() {
                                self.export();
                            }
                            if ui.button("⤵ Undo").clicked() {
                                self.stave.history.undo();
                            }
                            if ui.button("⤴ Redo").clicked() {
                                self.stave.history.redo();
                            }
                        });
                    })
                });
        });
    }
}

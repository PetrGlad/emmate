use crate::command_palette::CommandPalette;
use crate::commands::{self, Command};
use crate::common::Time;
use crate::engine::{Engine, EngineCommand, StatusEvent};
use crate::note_sampler::NoteTester;
use crate::project::Project;
use crate::stave::Stave;
use eframe::egui::{Key, Ui, Vec2};
use eframe::{self, CreationContext, egui};
use egui_extras::{Size, StripBuilder};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

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
    stop_app: Arc<AtomicBool>,
    note_tester: NoteTester,
    palette: CommandPalette,
}

impl EmApp {
    pub fn new(
        ctx: &CreationContext,
        engine_command_send: mpsc::Sender<Box<EngineCommand>>,
        stop_app: Arc<AtomicBool>,
        project: Project,
    ) -> EmApp {
        let (message_sender, message_receiver) = mpsc::channel();

        let app = EmApp {
            title: project.title,
            home_path: project.home_path,
            stave: Stave::new(project.history),
            engine_command_send: engine_command_send.to_owned(),
            message_receiver,
            follow_playback: false,
            stop_app,
            note_tester: NoteTester::new(engine_command_send.to_owned()),
            palette: CommandPalette::default(),
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

    fn execute(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::CommandPalette => self.palette.toggle(),
            Command::Pause => self.toggle_pause(),
            Command::Export => self.export(),
            Command::FollowPlayback => self.follow_playback = !self.follow_playback,
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::ZoomIn => egui::gui_zoom::zoom_in(ctx),
            Command::ZoomOut => egui::gui_zoom::zoom_out(ctx),
            Command::ZoomReset => ctx.set_zoom_factor(1.0),
            Command::ScrollPageLeft => self
                .stave
                .viewport
                .scroll_by(ctx.content_rect().width() / -4.0),
            Command::ScrollPageRight => self
                .stave
                .viewport
                .scroll_by(ctx.content_rect().width() / 4.0),
            _ => {
                if let Some(pos) = self.stave.execute(ctx, command) {
                    self.stave.move_cursor_to(pos);
                    self.engine_seek(pos);
                }
            }
        }
    }

    fn engine_seek(&self, to: Time) {
        self.engine_command_send
            .send(Box::new(move |engine| engine.seek(to)))
            .unwrap();
    }
}

impl eframe::App for EmApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if self.stop_app.load(std::sync::atomic::Ordering::Relaxed) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(message) = self.message_receiver.try_iter().last() {
            match message {
                Message::UpdateTime(t) => {
                    self.stave.cursor_position = t;
                    if self.follow_playback {
                        let at = self.stave.cursor_position;
                        self.stave.viewport.scroll_to(at, 0.1);
                    }
                }
            }
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let from_palette = self.palette.show(ui.ctx());
            if let Some(command) = from_palette.or_else(|| commands::pressed(ui.ctx())) {
                self.execute(ui.ctx(), command);
            }

            {
                let h = self.stave.history.read().expect("Read stave.history.");
                ui.heading(format!("🌲 {} [{}]", self.title, h.version()));
            }
            StripBuilder::new(ui)
                .size(Size::remainder())
                .size(Size::exact(20.0))
                .size(Size::exact(20.0))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        let stave_response = self.stave.show(ui);

                        if let Some(hover_pos) = stave_response.ui_response.hover_pos() {
                            let dz = ui.input(|i| i.zoom_delta());
                            if dz != 1.0 {
                                self.stave.viewport.zoom(dz, hover_pos.x);
                            }
                            let scroll_delta = ui.input(|i| i.smooth_scroll_delta);
                            if scroll_delta != Vec2::ZERO {
                                self.stave.viewport.scroll_by(scroll_delta.x);
                            }
                        }
                        // TODO (improvement) Handle also note-hovered (this requires to resolve note itself).
                        if let Some(pitch_hovered) = stave_response.pitch_hovered
                            && ui.input(|i| i.keys_down == HashSet::from([Key::T]))
                        {
                            self.note_tester.play_sample(&pitch_hovered);
                        }
                        if let Some(pos) = stave_response.new_cursor_position {
                            self.engine_seek(pos);
                        }
                    });
                    strip.cell(|ui| {
                        ui.horizontal(|ui| {
                            let mouse_x = ui.painter().clip_rect().min.x;
                            if ui.button(" + ").clicked() {
                                self.stave.viewport.zoom(1.05, mouse_x);
                            }
                            if ui.button(" - ").clicked() {
                                self.stave.viewport.zoom(1.0 / 1.05, mouse_x);
                            }
                            let scroll_step = ui.painter().clip_rect().size().x * 0.15;
                            if ui.button(" << ").clicked() {
                                self.stave.viewport.scroll_by(-scroll_step);
                            }
                            if ui.button(" >> ").clicked() {
                                self.stave.viewport.scroll_by(scroll_step);
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
                                self.execute(ui.ctx(), Command::Undo);
                            }
                            if ui.button("⤴ Redo").clicked() {
                                self.execute(ui.ctx(), Command::Redo);
                            }
                        });
                        ui.horizontal(|ui| {
                            // Status line
                            ui.label(format!(
                                "track_len={}  n_sel={}  t_sel={}  at={}s  fps={}",
                                self.stave
                                    .history
                                    .read()
                                    .expect("Read stave.history.")
                                    .with_track(|t| t.events.len()),
                                self.stave.note_selection.count(),
                                self.stave.time_selection.as_ref().map_or(
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
                                Duration::from_micros(self.stave.cursor_position as u64).as_secs(),
                                ui.input(|i| (1.0 / i.stable_dt) as u32)
                            ));
                        });
                    })
                });
        });
    }

    fn on_exit(&mut self) {
        self.engine_command_send
            .send(Box::new(move |engine| engine.stop()))
            .unwrap();
        log::info!("Bye.");
    }
}

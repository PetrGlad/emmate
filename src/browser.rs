// File browser with a read-only preview stave. Files are parsed into memory and are not
// imported into any project until they are opened for editing (in a separate emmate process).

use crate::app::navigate_stave;
use crate::common::Time;
use crate::engine::{EngineHandle, StatusEvent};
use crate::stave::Stave;
use crate::track::{Track, import_smf};
use crate::track_history::TrackHistory;
use crate::track_source::TrackSource;
use crate::util::IdSeq;
use eframe::egui::{self, Key, Ui};
use eframe::{CreationContext, egui::ScrollArea};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock, mpsc};
use std::time::Duration;
use sync_cow::SyncCow;

struct Entry {
    path: PathBuf,
    is_dir: bool,
}

pub struct BrowserApp {
    dir: PathBuf,
    entries: Vec<Entry>,
    selected: Option<PathBuf>,
    status: String,
    autoplay: bool,
    stave: Stave,
    track: Arc<SyncCow<Track>>,
    engine: EngineHandle,
    time_receiver: mpsc::Receiver<Time>,
    stop_app: Arc<AtomicBool>,
}

fn is_midi(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"))
}

/// Directories first, then MIDI files; both sorted by name.
fn list_dir(dir: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let hidden = e.file_name().to_string_lossy().starts_with('.');
                let is_dir = path.is_dir();
                (!hidden && (is_dir || is_midi(&path))).then_some(Entry { path, is_dir })
            })
            .collect(),
        Err(e) => {
            log::error!("Cannot list {:?}: {e}", dir);
            vec![]
        }
    };
    entries.sort_by(|a, b| {
        (!a.is_dir, a.path.file_name()).cmp(&(!b.is_dir, b.path.file_name()))
    });
    entries
}

/// Checks that would otherwise be panics in the importer.
fn check_importable(path: &Path) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let smf = midly::Smf::parse(&data).map_err(|e| e.to_string())?;
    if smf.header.format != midly::Format::SingleTrack {
        return Err(format!("unsupported MIDI format {:?}", smf.header.format));
    }
    if smf.tracks.is_empty() {
        return Err("no tracks".into());
    }
    Ok(())
}

impl BrowserApp {
    pub fn new(
        ctx: &CreationContext,
        engine: EngineHandle,
        stop_app: Arc<AtomicBool>,
        dir: PathBuf,
    ) -> BrowserApp {
        // The history is only a holder of the in-memory track here. Nothing is ever persisted.
        let history = TrackHistory::with_directory(&PathBuf::from("/nonexistent-preview"));
        let track = history.track.clone();
        let mut stave = Stave::new(Arc::new(RwLock::new(history)));
        stave.read_only = true;

        engine
            .send(Box::new({
                let source = TrackSource::new(track.clone());
                move |engine| {
                    engine.add(Box::new(source));
                    engine.set_paused(true);
                }
            }))
            .unwrap();

        let (sender, time_receiver) = mpsc::channel();
        let repaint_ctx = ctx.egui_ctx.clone();
        engine
            .send(Box::new(move |engine| {
                engine.set_status_receiver(Some(Box::new(move |ev| {
                    let StatusEvent::Time(t) = ev;
                    if sender.send(t).is_ok() {
                        repaint_ctx.request_repaint_after(Duration::from_micros(20_000));
                    }
                })));
            }))
            .unwrap();

        BrowserApp {
            entries: list_dir(&dir),
            dir,
            selected: None,
            status: String::new(),
            autoplay: true,
            stave,
            track,
            engine,
            time_receiver,
            stop_app,
        }
    }

    fn change_dir(&mut self, dir: PathBuf) {
        self.entries = list_dir(&dir);
        self.dir = dir;
    }

    fn preview(&mut self, path: &Path) {
        self.selected = Some(path.to_owned());
        if let Err(e) = check_importable(path) {
            self.status = format!("Cannot preview: {e}");
            return;
        }
        let events = import_smf(&IdSeq::new(0), &path.to_path_buf());
        let count = events.len();
        self.track.edit(|t| t.reset(events));
        self.status = format!("{count} events");
        self.stave.note_selection.clear();
        self.stave.time_selection = None;
        self.stave.zoom_to_fit(1_000_000);
        self.stave.cursor_position = 0;
        let autoplay = self.autoplay;
        self.engine
            .send(Box::new(move |engine| {
                engine.seek(0);
                engine.set_paused(!autoplay);
            }))
            .unwrap();
    }

    /// Step the selection among the files of the current directory.
    fn select_relative(&mut self, step: isize) {
        let files: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.path.clone())
            .collect();
        if files.is_empty() {
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|s| files.iter().position(|f| f == s));
        let next = match current {
            Some(i) => (i as isize + step).clamp(0, files.len() as isize - 1) as usize,
            None => 0,
        };
        self.preview(&files[next].clone());
    }

    /// Edit the file: one stave per process, so launch another emmate instance.
    fn open_for_editing(&mut self, path: &Path) {
        let result = std::env::current_exe()
            .and_then(|exe| std::process::Command::new(exe).arg("--midi-file").arg(path).spawn());
        if let Err(e) = result {
            self.status = format!("Cannot launch the editor: {e}");
        }
    }

    fn toggle_pause(&self) {
        self.engine
            .send(Box::new(|engine| engine.toggle_pause()))
            .unwrap();
    }
}

impl eframe::App for BrowserApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if self.stop_app.load(std::sync::atomic::Ordering::Relaxed) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(t) = self.time_receiver.try_iter().last() {
            self.stave.cursor_position = t;
        }

        let (down, up, space, enter) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Space),
                i.key_pressed(Key::Enter),
            )
        });
        if down {
            self.select_relative(1);
        }
        if up {
            self.select_relative(-1);
        }
        if space {
            self.toggle_pause();
        }
        if enter {
            if let Some(path) = self.selected.clone() {
                self.open_for_editing(&path);
            }
        }

        egui::Panel::left("files")
            .resizable(true)
            .default_size(280.0)
            .show_inside(ui, |ui| {
                ui.label(self.dir.to_string_lossy());
                let mut go_to = None;
                let mut to_preview = None;
                let mut to_open = None;
                ScrollArea::vertical().show(ui, |ui| {
                    if let Some(parent) = self.dir.parent() {
                        if ui.selectable_label(false, "⬆ ..").clicked() {
                            go_to = Some(parent.to_owned());
                        }
                    }
                    for e in &self.entries {
                        let name = e.path.file_name().unwrap_or_default().to_string_lossy();
                        if e.is_dir {
                            if ui.selectable_label(false, format!("📁 {name}")).clicked() {
                                go_to = Some(e.path.clone());
                            }
                        } else {
                            let selected = self.selected.as_ref() == Some(&e.path);
                            let response = ui.selectable_label(selected, format!("🎵 {name}"));
                            if selected && (down || up) {
                                response.scroll_to_me(None);
                            }
                            if response.double_clicked() {
                                to_open = Some(e.path.clone());
                            } else if response.clicked() {
                                to_preview = Some(e.path.clone());
                            }
                        }
                    }
                });
                if let Some(d) = go_to {
                    self.change_dir(d);
                }
                if let Some(p) = to_preview {
                    self.preview(&p);
                }
                if let Some(p) = to_open {
                    self.open_for_editing(&p);
                }
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.autoplay, "Autoplay");
                if ui.button(" ⏯ ").clicked() {
                    self.toggle_pause();
                }
                if ui.button(" ⏮ ").clicked() {
                    self.engine.send(Box::new(|engine| engine.seek(0))).unwrap();
                }
                if ui.button("🔇").clicked() {
                    self.engine
                        .send(Box::new(crate::engine::Engine::reset))
                        .unwrap();
                }
                ui.label(&self.status);
            });
            // Banner: says what this is and offers the way out of it.
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(255, 243, 205))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("👁 Preview, read-only. Editing opens the file in a project.");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let edit = egui::Button::new(
                                egui::RichText::new("✏  Edit this file")
                                    .size(16.0)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(30, 110, 200))
                            .min_size(egui::vec2(150.0, 30.0));
                            if ui.add_enabled(self.selected.is_some(), edit).clicked() {
                                if let Some(path) = self.selected.clone() {
                                    self.open_for_editing(&path);
                                }
                            }
                        });
                    });
                });
            let stave_response = self.stave.show(ui);
            let stave_rect = stave_response.ui_response.rect;
            ui.painter_at(stave_rect).text(
                stave_rect.right_bottom() - egui::vec2(16.0, 12.0),
                egui::Align2::RIGHT_BOTTOM,
                "PREVIEW",
                egui::FontId::proportional(48.0),
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 28),
            );
            navigate_stave(ui, &mut self.stave, &stave_response);
            if let Some(pos) = stave_response.new_cursor_position {
                self.engine
                    .send(Box::new(move |engine| engine.seek(pos)))
                    .unwrap();
            }
        });
    }

    fn on_exit(&mut self) {
        self.engine.send(Box::new(|engine| engine.stop())).unwrap();
    }
}

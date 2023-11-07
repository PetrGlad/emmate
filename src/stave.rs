use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Range;
use std::path::PathBuf;

use eframe::egui::{
    self, Color32, Frame, Margin, Modifiers, Painter, PointerButton, Pos2, Rangef, Rect, Rounding,
    Sense, Stroke, Ui,
};
use egui::Rgba;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::common::VersionId;
use crate::engine::TransportTime;
use crate::track::{
    EventId, Level, Note, Pitch, Track, TrackEvent, TrackEventType, MIDI_CC_SUSTAIN_ID,
};
use crate::track_history::{ActionId, TrackHistory};
use crate::{track, Pix};

pub type StaveTime = i64;

// Tone 60 is C3, tones start at C-2 (21).
const PIANO_LOWEST_KEY: Pitch = 21;
const PIANO_KEY_COUNT: Pitch = 88;
const PIANO_DAMPER_LINE: Pitch = PIANO_LOWEST_KEY - 1;
const PIANO_KEY_LINES: Range<Pitch> = PIANO_LOWEST_KEY..(PIANO_LOWEST_KEY + PIANO_KEY_COUNT);
// Lines including controller values placeholder.
const STAVE_KEY_LINES: Range<Pitch> = (PIANO_LOWEST_KEY - 1)..(PIANO_LOWEST_KEY + PIANO_KEY_COUNT);

fn key_line_ys(view_y_range: &Rangef, pitches: Range<Pitch>) -> (BTreeMap<Pitch, Pix>, Pix) {
    let mut lines = BTreeMap::new();
    let step = view_y_range.span() / pitches.len() as Pix;
    let mut y = view_y_range.max - step / 2.0;
    for p in pitches {
        lines.insert(p, y);
        y -= step;
    }
    (lines, step)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TimeSelection {
    pub from: StaveTime,
    pub to: StaveTime,
}

impl TimeSelection {
    pub fn is_empty(&self) -> bool {
        self.to - self.from <= 0
    }
}

#[derive(Debug)]
pub struct NoteDraw {
    time: TimeSelection,
    pitch: Pitch,
}

#[derive(Debug, Default)]
pub struct NotesSelection {
    selected: HashSet<EventId>,
}

impl NotesSelection {
    fn toggle(&mut self, id: &EventId) {
        if self.selected.contains(&id) {
            self.selected.remove(&id);
        } else {
            self.selected.insert(*id);
        }
    }

    fn contains(&self, ev: &TrackEvent) -> bool {
        self.selected.contains(&ev.id)
    }

    fn clear(&mut self) {
        self.selected.clear();
    }
}

fn to_transport_time(value: StaveTime) -> TransportTime {
    value.max(0) as TransportTime
}

impl From<&TimeSelection> for track::TimeSelection {
    fn from(value: &TimeSelection) -> Self {
        track::TimeSelection {
            from: to_transport_time(value.from),
            to: to_transport_time(value.to),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct Bookmark {
    at: StaveTime,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Bookmarks {
    // Maybe bookmarks should also be events in the track.
    pub list: BTreeSet<Bookmark>,
}

impl Bookmarks {
    pub fn load_from(file_path: &PathBuf) -> Bookmarks {
        let binary = std::fs::read(file_path)
            .expect(&*format!("load bookmarks from {}", &file_path.display()));
        rmp_serde::from_slice(&binary).expect("deserialize bookmarks")
    }

    pub fn store_to(&self, file_path: &PathBuf) {
        let mut binary = Vec::new();
        self.serialize(&mut rmp_serde::Serializer::new(&mut binary).with_struct_map())
            .expect("serialize bookmarks");
        std::fs::write(file_path, binary)
            .expect(&*format!("save bookmarks to {}", &file_path.display()));
    }
}

#[derive(Debug)]
pub struct Stave {
    pub history: TrackHistory,
    pub track_version: VersionId,

    pub time_left: StaveTime,
    pub time_right: StaveTime,
    pub view_rect: Rect,

    pub cursor_position: StaveTime,
    pub bookmarks: Bookmarks,
    pub time_selection: Option<TimeSelection>,
    pub note_draw: Option<NoteDraw>,
    pub note_selection: NotesSelection,
}

const COLOR_SELECTED: Rgba = Rgba::from_rgb(0.2, 0.5, 0.55);
const COLOR_HOVERED: Rgba = COLOR_SELECTED;

struct InnerResponse {
    response: egui::Response,
    pitch_hovered: Option<Pitch>,
    time_hovered: Option<StaveTime>,
    note_hovered: Option<EventId>,
    modifiers: Modifiers,
}

pub struct StaveResponse {
    pub ui_response: egui::Response,
    pub new_cursor_position: Option<StaveTime>,
}

impl Stave {
    pub fn new(history: TrackHistory) -> Stave {
        Stave {
            history,
            track_version: 0,
            time_left: 0,
            time_right: chrono::Duration::minutes(5).num_microseconds().unwrap(),
            view_rect: Rect::NOTHING,
            cursor_position: 0,
            bookmarks: Default::default(),
            time_selection: None,
            note_draw: None,
            note_selection: NotesSelection::default(),
        }
    }

    pub fn save_to(&mut self, file_path: &PathBuf) {
        self.history.with_track(|track| track.save_to(file_path));
    }

    pub fn load_from(&mut self, file_path: &PathBuf) {
        self.history
            .update_track(None, |track| track.load_from(file_path));
    }

    /// Pixel/uSec, can be cached.
    pub fn time_scale(&self) -> f32 {
        self.view_rect.width() / (self.time_right - self.time_left) as f32
    }

    pub fn x_from_time(&self, at: StaveTime) -> Pix {
        self.view_rect.min.x + (at as f32 - self.time_left as f32) * self.time_scale()
    }

    pub fn time_from_x(&self, x: Pix) -> StaveTime {
        self.time_left + ((x - self.view_rect.min.x) / self.time_scale()) as StaveTime
    }

    pub fn zoom(&mut self, zoom_factor: f32, mouse_x: Pix) {
        // Zoom so that position under mouse pointer stays put.
        let at = self.time_from_x(mouse_x);
        self.time_left = at - ((at - self.time_left) as f32 / zoom_factor) as StaveTime;
        self.time_right = at + ((self.time_right - at) as f32 / zoom_factor) as StaveTime;
    }

    pub fn scroll(&mut self, dt: StaveTime) {
        self.time_left += dt;
        self.time_right += dt;
    }

    pub fn scroll_by(&mut self, dx: Pix) {
        self.scroll((dx / self.time_scale()) as StaveTime);
    }

    pub fn scroll_to(&mut self, at: StaveTime, view_fraction: f32) {
        self.scroll(
            at - ((self.time_right - self.time_left) as f32 * view_fraction) as StaveTime
                - self.time_left,
        );
    }

    const NOTHING_ZONE: TimeSelection = TimeSelection {
        from: StaveTime::MIN,
        to: 0,
    };

    fn view(&mut self, ui: &mut Ui) -> InnerResponse {
        Frame::none()
            .inner_margin(Margin::symmetric(4.0, 4.0))
            .stroke(Stroke::NONE)
            .show(ui, |ui| {
                let bounds = ui.available_rect_before_wrap();
                self.view_rect = bounds;
                let (key_ys, half_tone_step) = key_line_ys(&bounds.y_range(), STAVE_KEY_LINES);
                let mut pitch_hovered = None;
                let mut time_hovered = None;
                let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                if let Some(pointer_pos) = pointer_pos {
                    pitch_hovered = Some(closest_pitch(&key_ys, pointer_pos));
                    time_hovered = Some(self.time_from_x(pointer_pos.x));
                }
                let painter = ui.painter_at(bounds);

                Self::draw_grid(&painter, bounds, &key_ys, &pitch_hovered);
                let selection_color = Color32::from_rgba_unmultiplied(64, 80, 100, 60);
                if let Some(s) = &self.time_selection {
                    self.draw_time_selection(&painter, &s, &selection_color);
                }
                self.draw_time_selection(
                    &painter,
                    &Stave::NOTHING_ZONE,
                    &Color32::from_black_alpha(15),
                );
                let mut note_hovered = None;
                {
                    let track = self.history.track.read().expect("Read track.");
                    self.draw_track(
                        &key_ys,
                        &half_tone_step,
                        &mut pitch_hovered,
                        &mut time_hovered,
                        &mut note_hovered,
                        &painter,
                        &track,
                    );
                    self.track_version = track.version;
                }

                self.draw_cursor(
                    &painter,
                    self.x_from_time(self.cursor_position),
                    Rgba::from_rgba_unmultiplied(0.1, 0.9, 0.1, 0.8).into(),
                );

                for &bm in &self.bookmarks.list {
                    self.draw_cursor(
                        &painter,
                        self.x_from_time(bm.at),
                        Rgba::from_rgba_unmultiplied(0.0, 0.4, 0.0, 0.3).into(),
                    );
                }

                if let Some(new_note) = &self.note_draw {
                    self.draw_note(
                        &painter,
                        64,
                        (new_note.time.from, new_note.time.to),
                        *key_ys.get(&new_note.pitch).unwrap(),
                        half_tone_step,
                        true,
                    );
                }

                InnerResponse {
                    response: ui.allocate_response(bounds.size(), Sense::click_and_drag()),
                    pitch_hovered,
                    time_hovered,
                    note_hovered,
                    modifiers: ui.input(|i| i.modifiers),
                }
            })
            .inner
    }

    fn draw_track(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        pitch_hovered: &Option<Pitch>,
        time_hovered: &Option<StaveTime>,
        note_hovered: &mut Option<EventId>,
        painter: &Painter,
        track: &Track,
    ) {
        let mut last_damper_value: (StaveTime, Level) = (0, 0);
        for i in 0..track.events.len() {
            let event = &track.events[i];
            match &event.event {
                TrackEventType::Note(note) => {
                    if let Some(y) = key_ys.get(&note.pitch) {
                        let is_hovered =
                            Self::event_hovered(&pitch_hovered, &time_hovered, event, &note.pitch);
                        if is_hovered {
                            note_hovered.replace(event.id);
                        }
                        self.draw_note(
                            &painter,
                            note.velocity,
                            (
                                event.at as StaveTime,
                                (event.at + note.duration) as StaveTime,
                            ),
                            *y,
                            *half_tone_step,
                            self.note_selection.contains(&event),
                        );
                    }
                }
                TrackEventType::Controller(v) if v.controller_id == MIDI_CC_SUSTAIN_ID => {
                    if let Some(y) = key_ys.get(&PIANO_DAMPER_LINE) {
                        let at = event.at as StaveTime;
                        self.draw_cc(
                            &painter,
                            last_damper_value.0,
                            at,
                            last_damper_value.1,
                            *y,
                            *half_tone_step,
                        );
                        last_damper_value = (at, v.value);
                    }
                }
                _ => (), /*println!(
                             "Not displaying event {:?}, the event type is not supported yet.",
                             event
                         )*/
            }
        }
    }

    pub fn show(&mut self, ui: &mut Ui) -> StaveResponse {
        let stave_response = self.view(ui);

        if let Some(note_id) = stave_response.note_hovered {
            let clicked = ui.input(|i| i.pointer.button_clicked(PointerButton::Primary));
            if clicked {
                self.note_selection.toggle(&note_id);
            }
        }

        let inner = &stave_response.response;
        self.update_note_draw(
            inner,
            &stave_response.modifiers,
            &stave_response.time_hovered,
            &stave_response.pitch_hovered,
        );
        self.update_time_selection(&inner, &stave_response.time_hovered);
        let new_cursor_position = self.handle_commands(&inner);
        if let Some(pos) = new_cursor_position {
            self.cursor_position = pos;
            self.ensure_visible(pos);
        }

        StaveResponse {
            ui_response: stave_response.response,
            new_cursor_position,
        }
    }

    fn event_hovered(
        pitch_hovered: &Option<Pitch>,
        time_hovered: &Option<StaveTime>,
        event: &TrackEvent,
        pitch: &Pitch,
    ) -> bool {
        if let Some(t) = &time_hovered {
            if let Some(p) = pitch_hovered {
                event.is_active(*t as TransportTime) && p == pitch
            } else {
                false
            }
        } else {
            false
        }
    }

    const KEYBOARD_TIME_STEP: StaveTime = 10_000;

    fn handle_commands(&mut self, response: &egui::Response) -> Option<StaveTime> {
        // Need to see if duplication here can be reduced.
        // Likely the dispatch needs some hash map that for each input state defines a unique command.
        // Need to support focus somehow so the commands only active when stave is focused.
        // Currently commands also affect other widgets (e.g. arrows change button focus).

        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::Q))
        }) {
            self.note_selection.clear();
        }

        // Tape insert/remove
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::NONE,
                egui::Key::Delete,
            ))
        }) {
            self.history.update_track(None, |track| {
                if let Some(time_selection) = &self.time_selection {
                    track.tape_cut(&time_selection.into());
                }
                track.delete_events(&self.note_selection.selected);
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::NONE,
                egui::Key::Insert,
            ))
        }) {
            self.history.update_track(None, |track| {
                if let Some(time_selection) = &self.time_selection {
                    track.tape_insert(&time_selection.into());
                }
            });
        }

        // Tail shift
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL | Modifiers::SHIFT,
                egui::Key::ArrowRight,
            ))
        }) {
            self.history
                .update_track(Some("tail_shift_right"), |track| {
                    track.shift_tail(
                        &(self.cursor_position as TransportTime),
                        Stave::KEYBOARD_TIME_STEP,
                    );
                });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL | Modifiers::SHIFT,
                egui::Key::ArrowLeft,
            ))
        }) {
            self.history.update_track(Some("tail_shift_left"), |track| {
                track.shift_tail(
                    &(self.cursor_position as TransportTime),
                    -Stave::KEYBOARD_TIME_STEP,
                );
            });
        }

        // Note time moves
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::ALT | Modifiers::SHIFT,
                egui::Key::ArrowRight,
            )) || i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::SHIFT, egui::Key::L))
        }) {
            self.history
                .update_track(Some("note_shift_right"), |track| {
                    track.shift_events(
                        &(|ev| self.note_selection.contains(ev)),
                        Stave::KEYBOARD_TIME_STEP,
                    );
                });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::ALT | Modifiers::SHIFT,
                egui::Key::ArrowLeft,
            )) || i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::SHIFT, egui::Key::H))
        }) {
            self.history.update_track(Some("note_shift_left"), |track| {
                track.shift_events(
                    &(|ev| self.note_selection.contains(ev)),
                    -Stave::KEYBOARD_TIME_STEP,
                );
            });
        }

        // Note edits
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::SHIFT, egui::Key::H))
        }) {
            self.edit_selected_notes(
                Some("note_duration_increase"),
                &(|note| {
                    note.duration = note
                        .duration
                        .checked_sub(Stave::KEYBOARD_TIME_STEP as TransportTime)
                        .unwrap_or(0);
                }),
            );
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::SHIFT, egui::Key::L))
        }) {
            self.edit_selected_notes(
                Some("note_duration_decrease"),
                &(|note| {
                    note.duration = note
                        .duration
                        .checked_add(Stave::KEYBOARD_TIME_STEP as TransportTime)
                        .unwrap_or(0);
                }),
            );
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::U))
        }) {
            self.edit_selected_notes(
                Some("note_pitch_up"),
                &(|note| {
                    if PIANO_KEY_LINES.contains(&(note.pitch + 1)) {
                        note.pitch += 1;
                    }
                }),
            );
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::J))
        }) {
            self.edit_selected_notes(
                Some("note_pitch_down"),
                &(|note| {
                    if PIANO_KEY_LINES.contains(&(note.pitch - 1)) {
                        note.pitch -= 1;
                    }
                }),
            );
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::I))
        }) {
            self.edit_selected_notes(
                Some("note_velocity_increase"),
                &(|note| {
                    note.velocity = note.velocity.checked_add(1).unwrap_or(Level::MAX);
                }),
            );
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::K))
        }) {
            self.edit_selected_notes(
                Some("note_velocity_decrease"),
                &(|note| {
                    note.velocity = note.velocity.checked_sub(1).unwrap_or(Level::MIN);
                }),
            );
        }

        // Undo/redo
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::CTRL, egui::Key::Z))
        }) {
            self.history.undo();
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::CTRL, egui::Key::Y))
                || i.consume_shortcut(&egui::KeyboardShortcut::new(
                    Modifiers::CTRL | Modifiers::SHIFT,
                    egui::Key::Z,
                ))
        }) {
            self.history.redo();
        }

        // Bookmarks & time navigation
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::M))
        }) {
            self.bookmarks.list.insert(Bookmark {
                at: self.cursor_position,
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::N))
        }) {
            self.bookmarks.list.remove(&Bookmark {
                at: self.cursor_position,
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL,
                egui::Key::ArrowLeft,
            ))
        }) {
            // Previous bookmark
            match self
                .bookmarks
                .list
                .iter()
                .rev()
                .find(|&bm| bm.at < self.cursor_position)
            {
                Some(bm) => return Some(bm.at),
                None => return Some(0),
            }
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL,
                egui::Key::ArrowRight,
            ))
        }) {
            // Next bookmark
            match self
                .bookmarks
                .list
                .iter()
                .find(|&bm| bm.at > self.cursor_position)
            {
                Some(bm) => return Some(bm.at),
                None => {
                    // To the end
                    return Some(self.history.with_track(|track| track.max_time()) as StaveTime);
                }
            }
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL,
                egui::Key::Home,
            ))
        }) {
            return Some(0);
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL,
                egui::Key::End,
            ))
        }) {
            // To the end
            return Some(self.history.with_track(|track| track.max_time()) as StaveTime);
        }
        if let Some(hover_pos) = response.hover_pos() {
            if response.middle_clicked() {
                let at = self.time_from_x(hover_pos.x);
                return Some(at);
            }
        }

        None
    }

    pub fn edit_selected_notes<Action: Fn(&mut Note)>(
        &mut self,
        action_id: ActionId,
        action: &Action,
    ) {
        self.history.update_track(action_id, |track| {
            track.edit_events(
                &(|ev| {
                    if self.note_selection.contains(ev) {
                        if let TrackEventType::Note(note) = &mut ev.event {
                            Some(note)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }),
                action,
            );
        });
    }

    fn update_time_selection(&mut self, response: &egui::Response, time: &Option<StaveTime>) {
        let drag_button = PointerButton::Primary;
        if response.clicked_by(drag_button) {
            self.time_selection = None;
        } else if response.drag_started_by(drag_button) {
            if let Some(time) = time {
                self.time_selection = Some(TimeSelection {
                    from: *time,
                    to: *time,
                });
            }
        } else if response.drag_released_by(drag_button) {
            // Just documenting how it can be handled
        } else if response.dragged_by(drag_button) {
            if let Some(time) = time {
                if let Some(selection) = &mut self.time_selection {
                    selection.to = *time;
                }
            }
        }
    }

    fn update_note_draw(
        &mut self,
        response: &egui::Response,
        modifiers: &Modifiers,
        time: &Option<StaveTime>,
        pitch: &Option<Pitch>,
    ) {
        // TODO Extract the drag pattern? See also update_time_selection.
        //      See how egui can help, there seem to be already some drag&drop support.
        let drag_button = PointerButton::Middle;
        if response.clicked_by(drag_button) {
            self.note_draw = None;
        } else if response.drag_started_by(drag_button) {
            if let Some(time) = time {
                if let Some(pitch) = pitch {
                    self.note_draw = Some(NoteDraw {
                        time: TimeSelection {
                            from: *time,
                            to: *time,
                        },
                        pitch: *pitch,
                    });
                }
            }
        } else if response.drag_released_by(drag_button) {
            dbg!("drag_released", &self.note_draw);
            if let Some(draw) = &mut self.note_draw {
                if !draw.time.is_empty() {
                    self.history.update_track(None, |track| {
                        let time_range = (
                            draw.time.from as TransportTime,
                            draw.time.to as TransportTime,
                        );
                        if draw.pitch == PIANO_DAMPER_LINE {
                            if modifiers.alt {
                                track.set_damper_to(time_range, false);
                            } else {
                                track.set_damper_to(time_range, true);
                            }
                        } else {
                            track.add_note(time_range, draw.pitch, 64);
                        }
                    });
                }
            }
            self.note_draw = None;
        } else if response.dragged_by(drag_button) {
            if let Some(time) = time {
                if let Some(draw) = &mut self.note_draw {
                    draw.time.to = *time;
                }
            }
        }
    }

    fn draw_cursor(&self, painter: &Painter, x: Pix, color: Color32) {
        painter.vline(
            x,
            painter.clip_rect().y_range(),
            Stroke { width: 2.0, color },
        )
    }

    fn draw_note(
        &self,
        painter: &Painter,
        velocity: Level,
        x_range: (StaveTime, StaveTime),
        y: Pix,
        height: Pix,
        selected: bool,
    ) {
        let paint_rect = Rect {
            min: Pos2 {
                x: self.x_from_time(x_range.0),
                y: y - height * 0.45,
            },
            max: Pos2 {
                x: self.x_from_time(x_range.1),
                y: y + height * 0.45,
            },
        };
        let stroke_color = note_color(&velocity, selected);
        painter.rect_filled(paint_rect, Rounding::ZERO, stroke_color);
    }

    fn draw_cc(
        &self,
        painter: &Painter,
        last_time: StaveTime,
        at: StaveTime,
        value: Level,
        y: Pix,
        height: Pix,
    ) {
        self.draw_note(painter, value, (last_time, at), y, height, false)
    }

    fn draw_grid(
        painter: &Painter,
        bounds: Rect,
        keys: &BTreeMap<Pitch, Pix>,
        pitch_hovered: &Option<Pitch>,
    ) {
        for (pitch, y) in keys {
            let mut color = if is_black_key(&pitch) {
                Rgba::from_rgb(0.05, 0.05, 0.05)
            } else {
                Rgba::from_rgb(0.55, 0.55, 0.55)
            };
            if let Some(p) = pitch_hovered {
                if pitch == p {
                    color = COLOR_HOVERED
                }
            }
            painter.hline(
                bounds.min.x..=bounds.max.x,
                *y,
                Stroke {
                    width: 1.0,
                    color: color.into(),
                },
            );
        }
    }

    pub fn draw_time_selection(
        &self,
        painter: &Painter,
        selection: &TimeSelection,
        color: &Color32,
    ) {
        let clip = painter.clip_rect();
        let area = Rect {
            min: Pos2 {
                x: self.x_from_time(selection.from),
                y: clip.min.y,
            },
            max: Pos2 {
                x: self.x_from_time(selection.to),
                y: clip.max.y,
            },
        };
        painter.rect_filled(area, Rounding::ZERO, *color);
        painter.vline(
            area.min.x,
            clip.y_range(),
            Stroke {
                width: 1.0,
                color: color.gamma_multiply(2.0),
            },
        );
        painter.vline(
            area.max.x,
            clip.y_range(),
            Stroke {
                width: 1.0,
                color: color.gamma_multiply(2.0),
            },
        )
    }

    fn ensure_visible(&mut self, at: StaveTime) {
        let x_range = self.view_rect.x_range();
        let x = self.x_from_time(at);
        if !x_range.contains(x) {
            if x_range.max < x {
                self.scroll_to(at, 0.8);
            } else {
                self.scroll_to(at, 0.2);
            }
        }
    }
}

fn is_black_key(tone: &Pitch) -> bool {
    vec![1, 3, 6, 8, 10].contains(&(tone % 12))
}

fn closest_pitch(pitch_ys: &BTreeMap<Pitch, Pix>, pointer_pos: Pos2) -> Pitch {
    *pitch_ys
        .iter()
        .min_by_key(|(_, &y)| OrderedFloat((y - pointer_pos.y).abs()))
        .unwrap()
        .0
}

fn note_color(velocity: &Level, selected: bool) -> Color32 {
    let c = if selected {
        COLOR_SELECTED
    } else {
        Rgba::from_rgb(0.6, 0.7, 0.7)
    };
    egui::lerp(c..=Rgba::from_rgb(0.0, 0.0, 0.0), *velocity as f32 / 128.0).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_serialization() {
        let file_path = PathBuf::from("./target/test_bookmarks_serialization");
        let bm1 = Bookmark { at: 12 };
        let bm2 = Bookmark { at: 23 };

        let mut bookmarks = Bookmarks::default();
        bookmarks.list.insert(bm1);
        bookmarks.list.insert(bm2);
        bookmarks.store_to(&file_path);

        let loaded = Bookmarks::load_from(&file_path);
        assert_eq!(loaded.list.len(), 2);
        assert!(loaded.list.contains(&bm1));
        assert!(loaded.list.contains(&bm2));
    }
}

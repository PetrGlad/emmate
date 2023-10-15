use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use eframe::egui::{
    self, Color32, Frame, Key, Margin, Painter, PointerButton, Pos2, Rangef, Rect, Response,
    Rounding, Sense, Stroke, Ui,
};
use egui::Rgba;
use ordered_float::OrderedFloat;
use toml::value::Time;

use crate::engine::TransportTime;
use crate::lane::{
    EventId, Lane, LaneEvent, LaneEventType, Level, Note, Pitch, MIDI_CC_SUSTAIN_ID,
};
use crate::{lane, Pix};

pub type StaveTime = i64;

// Tone 60 is C3, tones start at C-2 (21)
const PIANO_LOWEST_KEY: Pitch = 21;
const PIANO_KEY_COUNT: Pitch = 88;
const PIANO_DAMPER_LINE: Pitch = PIANO_LOWEST_KEY - 1;
const PIANO_KEY_LINES: Range<Pitch> = PIANO_LOWEST_KEY..(PIANO_LOWEST_KEY + PIANO_KEY_COUNT);
// Lines including controller values placeholder
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
        self.to - self.from > 0
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

    fn contains(&self, ev: &LaneEvent) -> bool {
        self.selected.contains(&ev.id)
    }
}

fn to_transport_time(value: StaveTime) -> TransportTime {
    value.max(0) as TransportTime
}

impl From<&TimeSelection> for lane::TimeSelection {
    fn from(value: &TimeSelection) -> Self {
        lane::TimeSelection {
            from: to_transport_time(value.from),
            to: to_transport_time(value.to),
        }
    }
}

#[derive(Debug)]
pub struct Stave {
    pub track: Arc<RwLock<Lane>>,
    pub time_left: StaveTime,
    pub time_right: StaveTime,
    pub view_rect: Rect,
    pub cursor_position: StaveTime,

    pub time_selection: Option<TimeSelection>,
    pub note_draw: Option<NoteDraw>,
    pub note_selection: NotesSelection,
}

impl PartialEq for Stave {
    fn eq(&self, other: &Self) -> bool {
        // This eq implementation helps so egui knows when not to re-render.
        self.time_left == other.time_left
            && self.time_right == other.time_right
            && self.cursor_position == other.cursor_position
            && self.view_rect == other.view_rect
    }
}

const COLOR_SELECTED: Rgba = Rgba::from_rgb(0.2, 0.5, 0.55);
const COLOR_HOVERED: Rgba = COLOR_SELECTED;

pub struct StaveUiResponse {
    response: Response,
    pitch_hovered: Option<Pitch>,
    time_hovered: Option<StaveTime>,
    note_hovered: Option<EventId>,
}

impl Stave {
    pub fn new(track: Arc<RwLock<Lane>>) -> Stave {
        Stave {
            track: track.clone(),
            time_left: 0,
            time_right: chrono::Duration::minutes(5).num_microseconds().unwrap(),
            view_rect: Rect::NOTHING,
            cursor_position: 0,
            time_selection: None,
            note_draw: None,
            note_selection: NotesSelection::default(),
        }
    }

    pub fn save_to(&self, file_path: &PathBuf) {
        self.track
            .read()
            .expect("Cannot read track.")
            .save_to(file_path);
    }

    pub fn load_from(&self, file_path: &PathBuf) -> bool {
        self.track
            .write()
            .expect("Cannot read track.")
            .load_from(file_path)
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
        // Zoom so that position under mouse pointer stays in place.
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

    pub fn scroll_to(&mut self, at: StaveTime) {
        self.scroll(
            at - ((self.time_right - self.time_left) as f32 * 0.1) as StaveTime - self.time_left,
        );
    }

    const NOTHING_ZONE: TimeSelection = TimeSelection {
        from: StaveTime::MIN,
        to: 0,
    };

    // (Widget would require fn ui(self, ui: &mut Ui) -> Response)
    pub fn view(&mut self, ui: &mut Ui) -> StaveUiResponse {
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
                let track = self.track.read().expect("Cannot read track.");
                let mut last_damper_value: (StaveTime, Level) = (0, 0);
                let mut note_hovered = None;
                for i in 0..track.events.len() {
                    let event = &track.events[i];
                    match &event.event {
                        LaneEventType::Note(note) => {
                            if let Some(y) = key_ys.get(&note.pitch) {
                                let is_hovered = Self::event_hovered(
                                    &pitch_hovered,
                                    &time_hovered,
                                    event,
                                    &note.pitch,
                                );
                                if is_hovered {
                                    note_hovered = Some(&event.id);
                                }
                                self.draw_note(
                                    &painter,
                                    note.velocity,
                                    (
                                        event.at as StaveTime,
                                        (event.at + note.duration) as StaveTime,
                                    ),
                                    *y,
                                    half_tone_step,
                                    self.note_selection.contains(&event),
                                );
                            }
                        }
                        LaneEventType::Controller(v) if v.controller_id == MIDI_CC_SUSTAIN_ID => {
                            if let Some(y) = key_ys.get(&PIANO_DAMPER_LINE) {
                                let at = event.at as StaveTime;
                                self.draw_cc(
                                    &painter,
                                    last_damper_value,
                                    at,
                                    v.value,
                                    *y,
                                    half_tone_step,
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

                self.draw_cursor(
                    &painter,
                    self.x_from_time(self.cursor_position),
                    Rgba::from_rgba_unmultiplied(0.1, 0.7, 0.1, 0.7).into(),
                );

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

                StaveUiResponse {
                    response: ui.allocate_response(bounds.size(), Sense::click_and_drag()),
                    pitch_hovered,
                    time_hovered,
                    note_hovered: note_hovered.copied(),
                }
            })
            .inner
    }

    pub fn show(&mut self, ui: &mut Ui) -> Response {
        let stave_response = self.view(ui);

        if let Some(note_id) = stave_response.note_hovered {
            let clicked = ui.input(|i| i.pointer.button_clicked(PointerButton::Primary));
            if clicked {
                self.note_selection.toggle(&note_id);
            }
        }

        let inner = stave_response.response;
        self.update_note_draw(
            &inner,
            &stave_response.time_hovered,
            &stave_response.pitch_hovered,
        );
        self.update_time_selection(&inner, &stave_response.time_hovered);
        self.handle_commands(&inner);

        inner
    }

    fn event_hovered(
        pitch_hovered: &Option<Pitch>,
        time_hovered: &Option<StaveTime>,
        event: &LaneEvent,
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

    fn handle_commands(&mut self, response: &Response) {
        // Need to see if duplication here can be reduced.
        // Likely the dispatch needs some hash map that for each input state defines a unique command.
        // Need to support focus somehow so the commans only active when stave is focused.
        // Currently commands also affect other widgets (e.g. arrows change button focus).

        // Tape insert/remove
        if response.ctx.input(|i| i.key_pressed(Key::Delete)) {
            let mut track = self.track.write().expect("Cannot write to track.");
            if let Some(time_selection) = &self.time_selection {
                track.tape_cut(&time_selection.into());
            }
            track.delete_events(&self.note_selection.selected);
        }
        if response.ctx.input(|i| i.key_pressed(Key::Insert)) {
            let mut track = self.track.write().expect("Cannot write to track.");
            if let Some(time_selection) = &self.time_selection {
                track.tape_insert(&time_selection.into());
            }
        }

        // Tail shift
        if response
            .ctx
            .input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::ArrowRight))
        {
            let mut track = self.track.write().expect("Cannot write to track.");
            track.shift_tail(
                &(self.cursor_position as TransportTime),
                Stave::KEYBOARD_TIME_STEP,
            );
        }
        if response
            .ctx
            .input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::ArrowLeft))
        {
            let mut track = self.track.write().expect("Cannot write to track.");
            track.shift_tail(
                &(self.cursor_position as TransportTime),
                -Stave::KEYBOARD_TIME_STEP,
            );
        }

        // Note time moves
        if response.ctx.input(|i| {
            (i.modifiers.alt && i.modifiers.shift && i.key_pressed(Key::ArrowRight))
                || (i.modifiers.shift && i.key_pressed(Key::L))
        }) {
            let mut track = self.track.write().expect("Cannot write to track.");
            track.shift_events(
                &(|ev| self.note_selection.contains(ev)),
                Stave::KEYBOARD_TIME_STEP,
            );
        }
        if response.ctx.input(|i| {
            (i.modifiers.alt && i.modifiers.shift && i.key_pressed(Key::ArrowLeft))
                || (i.modifiers.shift && i.key_pressed(Key::H))
        }) {
            let mut track = self.track.write().expect("Cannot write to track.");
            track.shift_events(
                &(|ev| self.note_selection.contains(ev)),
                -Stave::KEYBOARD_TIME_STEP,
            );
        }

        // Note edits
        if response
            .ctx
            .input(|i| !i.modifiers.shift && i.key_pressed(Key::H))
        {
            self.edit_selected_notes(
                &(|note| {
                    note.duration = note
                        .duration
                        .checked_sub(Stave::KEYBOARD_TIME_STEP as TransportTime)
                        .unwrap_or(0);
                }),
            );
        }
        if response
            .ctx
            .input(|i| !i.modifiers.shift && i.key_pressed(Key::L))
        {
            self.edit_selected_notes(
                &(|note| {
                    note.duration = note
                        .duration
                        .checked_add(Stave::KEYBOARD_TIME_STEP as TransportTime)
                        .unwrap_or(0);
                }),
            );
        }
        if response.ctx.input(|i| i.key_pressed(Key::U)) {
            self.edit_selected_notes(
                &(|note| {
                    if PIANO_KEY_LINES.contains(&(note.pitch + 1)) {
                        note.pitch += 1;
                    }
                }),
            );
        }
        if response.ctx.input(|i| i.key_pressed(Key::J)) {
            self.edit_selected_notes(
                &(|note| {
                    if PIANO_KEY_LINES.contains(&(note.pitch - 1)) {
                        note.pitch -= 1;
                    }
                }),
            );
        }
        if response.ctx.input(|i| i.key_pressed(Key::I)) {
            self.edit_selected_notes(
                &(|note| {
                    note.velocity = note.velocity.checked_add(1).unwrap_or(Level::MAX);
                }),
            );
        }
        if response.ctx.input(|i| i.key_pressed(Key::K)) {
            self.edit_selected_notes(
                &(|note| {
                    note.velocity = note.velocity.checked_sub(1).unwrap_or(Level::MIN);
                }),
            );
        }
    }

    pub fn edit_selected_notes<Action: Fn(&mut Note)>(&mut self, action: &Action) {
        let mut track = self.track.write().expect("Cannot write to track.");
        track.edit_events(
            &(|ev| {
                if self.note_selection.contains(ev) {
                    if let LaneEventType::Note(note) = &mut ev.event {
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
    }

    fn update_time_selection(&mut self, response: &Response, time: &Option<StaveTime>) {
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
        response: &Response,
        time: &Option<StaveTime>,
        pitch: &Option<Pitch>,
    ) {
        // TODO Extract the drag procedure? See also update_time_selection.
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
            // TODO (implement) Add the note or CC to the lane.
            if let Some(draw) = &mut self.note_draw {
                if let Ok(track) = &mut self.track.try_write() {
                    let time_range = (
                        draw.time.from as TransportTime,
                        draw.time.to as TransportTime,
                    );
                    if draw.pitch == PIANO_DAMPER_LINE {
                        // TODO Need both: setting "on" and "off" range.
                        track.set_damper_range(time_range, true);
                        todo!();
                    } else if draw.time.is_empty() {
                        track.add_note(time_range, draw.pitch, 64);
                    }
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
        last_value: (StaveTime, Level),
        at: StaveTime,
        value: Level,
        y: Pix,
        height: Pix,
    ) {
        self.draw_note(painter, value, (last_value.0, at), y, height, false)
    }

    fn draw_grid(
        painter: &Painter,
        bounds: Rect,
        keys: &BTreeMap<Pitch, Pix>,
        pitch_hovered: &Option<Pitch>,
    ) {
        let is_black_key = |tone: &Pitch| vec![1, 3, 6, 8, 10].contains(&(tone % 12));
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

// Could not find a simple library for this.
fn ranges_intersect<T: Ord>(from_a: T, to_a: T, from_b: T, to_b: T) -> bool {
    from_a < to_b && from_b < to_a
}

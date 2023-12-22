use std::cell::RefCell;
use std::collections::btree_set::Iter;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Range;
use std::path::PathBuf;

use eframe::egui::{
    self, Color32, Context, Frame, Margin, Modifiers, Painter, PointerButton, Pos2, Rangef, Rect,
    Rounding, Sense, Stroke, Ui,
};
use egui::Rgba;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::changeset::{Changeset, EventActionsList};
use crate::common::Time;
use crate::track::{
    export_smf, ControllerSetValue, EventId, Level, Note, Pitch, Track, TrackEvent, TrackEventType,
    MAX_LEVEL, MIDI_CC_SUSTAIN_ID,
};
use crate::track_edit::{
    accent_selected_notes, add_new_note, delete_selected, set_damper, shift_selected, shift_tail,
    stretch_selected_notes, tape_delete, tape_insert, transpose_selected_notes, AppliedCommand,
    EditCommandId,
};
use crate::track_history::TrackHistory;
use crate::{util, Pix};

// Tone 60 is C3, tones start at C-2 (21).
const PIANO_LOWEST_KEY: Pitch = 21;
const PIANO_KEY_COUNT: Pitch = 88;
const PIANO_DAMPER_LINE: Pitch = PIANO_LOWEST_KEY - 1;
pub(crate) const PIANO_KEY_LINES: Range<Pitch> =
    PIANO_LOWEST_KEY..(PIANO_LOWEST_KEY + PIANO_KEY_COUNT);
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

#[derive(Debug, Clone)]
pub struct NoteDraw {
    time: Range<Time>,
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

    pub fn count(&self) -> usize {
        self.selected.len()
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct Bookmark {
    at: Time,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Bookmarks {
    // Maybe bookmarks should also be events in the track.
    pub list: BTreeSet<Bookmark>,
    file_path: PathBuf,
}

impl Bookmarks {
    pub fn new(file_path: &PathBuf) -> Bookmarks {
        Bookmarks {
            list: BTreeSet::default(),
            file_path: file_path.to_owned(),
        }
    }

    pub fn set(&mut self, at: Time) {
        self.list.insert(Bookmark { at });
        self.store_to(&self.file_path);
    }

    pub fn remove(&mut self, at: &Time) {
        self.list.remove(&Bookmark { at: *at });
        self.store_to(&self.file_path);
    }

    pub fn previous(&self, here: &Time) -> Option<Time> {
        self.list
            .iter()
            .rev()
            .find(|&bm| bm.at < *here)
            .map(|bm| bm.at)
    }

    pub fn next(&self, here: &Time) -> Option<Time> {
        self.list.iter().find(|&bm| bm.at > *here).map(|bm| bm.at)
    }

    pub fn iter(&self) -> Iter<Bookmark> {
        self.list.iter()
    }

    pub fn load_from(&mut self, file_path: &PathBuf) {
        self.list = util::load(file_path);
    }

    pub fn store_to(&self, file_path: &PathBuf) {
        util::store(&self.list, file_path);
    }
}

#[derive(Debug)]
pub struct EditTransition {
    pub animation_id: egui::Id,
    pub command_id: EditCommandId,
    pub changeset: Changeset,
    pub coeff: f32,
}

impl EditTransition {
    pub fn start(
        ctx: &Context,
        animation_id: egui::Id,
        command_id: EditCommandId,
        changeset: Changeset,
    ) -> Self {
        let coeff = ctx.animate_bool(animation_id, false);
        EditTransition {
            animation_id,
            command_id,
            coeff,
            changeset,
        }
    }

    pub fn update(mut self, ctx: &Context) -> Self {
        // DEBUG self.coeff = ctx.animate_bool(self.animation_id, true);
        self.coeff = ctx.animate_bool_with_time(self.animation_id, true, 1.0); // DEBUG
        self
    }

    pub fn value(&self) -> Option<f32> {
        if self.coeff >= 1.0 {
            None
        } else {
            Some(self.coeff)
        }
    }
}

#[derive(Debug)]
pub struct Stave {
    pub history: RefCell<TrackHistory>,

    pub time_left: Time,
    pub time_right: Time,
    pub view_rect: Rect,

    pub cursor_position: Time,
    pub bookmarks: Bookmarks,
    pub time_selection: Option<Range<Time>>,
    pub note_draw: Option<NoteDraw>,
    pub note_selection: NotesSelection,
    pub transition: Option<EditTransition>,
}

const COLOR_SELECTED: Rgba = Rgba::from_rgb(0.7, 0.1, 0.3);
const COLOR_HOVERED: Rgba = Rgba::from_rgb(0.2, 0.5, 0.55);

struct InnerResponse {
    response: egui::Response,
    pitch_hovered: Option<Pitch>,
    time_hovered: Option<Time>,
    note_hovered: Option<EventId>,
    modifiers: Modifiers,
}

pub struct StaveResponse {
    pub ui_response: egui::Response,
    pub new_cursor_position: Option<Time>,
}

impl Stave {
    pub fn new(history: RefCell<TrackHistory>, bookmarks: Bookmarks) -> Stave {
        Stave {
            history,
            time_left: 0,
            time_right: chrono::Duration::minutes(5).num_microseconds().unwrap(),
            view_rect: Rect::NOTHING,
            cursor_position: 0,
            bookmarks,
            time_selection: None,
            note_draw: None,
            note_selection: NotesSelection::default(),
            transition: None,
        }
    }

    pub fn save_to(&mut self, file_path: &PathBuf) {
        self.history
            .borrow()
            .with_track(|track| export_smf(&track.events, file_path));
    }

    /// Pixel/uSec, can be cached.
    pub fn time_scale(&self) -> f32 {
        self.view_rect.width() / (self.time_right - self.time_left) as f32
    }

    pub fn x_from_time(&self, at: Time) -> Pix {
        self.view_rect.min.x + (at as f32 - self.time_left as f32) * self.time_scale()
    }

    pub fn time_from_x(&self, x: Pix) -> Time {
        self.time_left + ((x - self.view_rect.min.x) / self.time_scale()) as Time
    }

    pub fn zoom(&mut self, zoom_factor: f32, mouse_x: Pix) {
        // Zoom so that position under mouse pointer stays put.
        // TODO (cleanup) Consider using emath::remap
        let at = self.time_from_x(mouse_x);
        self.time_left = at - ((at - self.time_left) as f32 / zoom_factor) as Time;
        self.time_right = at + ((self.time_right - at) as f32 / zoom_factor) as Time;
    }

    pub fn scroll(&mut self, dt: Time) {
        self.time_left += dt;
        self.time_right += dt;
    }

    pub fn scroll_by(&mut self, dx: Pix) {
        self.scroll((dx / self.time_scale()) as Time);
    }

    pub fn scroll_to(&mut self, at: Time, view_fraction: f32) {
        self.scroll(
            at - ((self.time_right - self.time_left) as f32 * view_fraction) as Time
                - self.time_left,
        );
    }

    const NOTHING_ZONE: Range<Time> = Time::MIN..0;

    fn view(&mut self, ui: &mut Ui) -> InnerResponse {
        Frame::none()
            .inner_margin(Margin::symmetric(4.0, 4.0))
            .stroke(Stroke::NONE)
            .show(ui, |ui| {
                let bounds = ui.available_rect_before_wrap();
                let egui_response = ui.allocate_response(bounds.size(), Sense::click_and_drag());
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
                let mut should_be_visible = None;
                {
                    let history = self.history.borrow();
                    let track = history.track.read().expect("Read track.");
                    should_be_visible = self.draw_track(
                        &key_ys,
                        &half_tone_step,
                        &mut pitch_hovered,
                        &mut time_hovered,
                        &mut note_hovered,
                        &painter,
                        &track,
                    );
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
                    self.default_draw_note(
                        &painter,
                        64,
                        (new_note.time.start, new_note.time.end),
                        *key_ys.get(&new_note.pitch).unwrap(),
                        half_tone_step,
                        true,
                    );
                }

                if let Some(range) = should_be_visible {
                    if !self.is_visible(range.0) && !self.is_visible(range.1) {
                        self.ensure_visible(range.0);
                    }
                }

                InnerResponse {
                    response: egui_response,
                    pitch_hovered,
                    time_hovered,
                    note_hovered: note_hovered,
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
        time_hovered: &Option<Time>,
        note_hovered: &mut Option<EventId>,
        painter: &Painter,
        track: &Track,
    ) -> Option<util::Range<Time>> {
        let mut last_damper_value: (Time, Level) = (0, 0);
        let x_range = painter.clip_rect().x_range();
        let mut selection_hints_left: HashSet<Pitch> = HashSet::new();
        let mut selection_hints_right: HashSet<Pitch> = HashSet::new();
        let mut should_be_visible = None;
        for i in 0..track.events.len() {
            let event = &track.events[i];
            match &event.event {
                TrackEventType::Note(note) => {
                    if Self::event_hovered(&pitch_hovered, &time_hovered, &event, &note.pitch) {
                        *note_hovered = Some(event.id);
                    }
                    if self.note_selection.contains(&event) {
                        // TODO If the affected notes are currently selected, they stay out-of-view after
                        //   the command. They should be made visible (or at least the selection hints should
                        //   be highlighted).
                        if x_range.max < self.x_from_time(event.at) {
                            selection_hints_right.insert(note.pitch);
                            continue;
                        } else if self.x_from_time(event.at + note.duration) < x_range.min {
                            selection_hints_left.insert(note.pitch);
                            continue;
                        }
                    }
                    self.draw_track_note(
                        key_ys,
                        half_tone_step,
                        &painter,
                        &mut should_be_visible,
                        &event,
                        &note,
                    );
                }
                TrackEventType::Controller(cc) => self.draw_track_cc(
                    &key_ys,
                    half_tone_step,
                    &painter,
                    &mut should_be_visible,
                    &mut last_damper_value,
                    &event,
                    &cc,
                ),
                _ => println!(
                    "Not displaying event {:?}, the event type is not supported yet.",
                    event
                ),
            }
        }
        draw_selection_hints(
            &painter,
            &key_ys,
            &half_tone_step,
            x_range.min,
            &selection_hints_left,
        );
        draw_selection_hints(
            &painter,
            &key_ys,
            &half_tone_step,
            x_range.max,
            &selection_hints_right,
        );
        should_be_visible
    }

    fn note_animation_params(ev: Option<&TrackEvent>) -> Option<((Time, Time), Pitch, Level)> {
        ev.and_then(|ev| {
            if let TrackEventType::Note(n) = &ev.event {
                Some(((ev.at, ev.at + n.duration), n.pitch, n.velocity))
            } else {
                None // CC is animated separately.
            }
        })
    }

    fn draw_track_note(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        painter: &Painter,
        should_be_visible: &mut Option<util::Range<Time>>,
        event: &TrackEvent,
        note: &Note,
    ) {
        if let Some(y) = key_ys.get(&note.pitch) {
            let is_selected = self.note_selection.contains(&event);
            let mut color = note_color(&note.velocity, is_selected);
            let mut t1 = event.at;
            let mut t2 = event.at + note.duration;
            let mut y = *y;
            if let Some(trans) = &self.transition {
                if let Some(change) = trans.changeset.changes.get(&event.id) {
                    // Interpolate the note states.

                    let coeff = trans.value().unwrap();
                    let a = Stave::note_animation_params(change.before());
                    let b = Stave::note_animation_params(change.after());

                    let ((t1_a, t2_a), p_a, v_a) = a.or(b).unwrap();
                    let ((t1_b, t2_b), p_b, v_b) = b.or(a).unwrap();

                    *should_be_visible = should_be_visible
                        .map(|(a, b)| (a.min(t1_a), b.max(t2_a)))
                        .or(Some((t1_a, t2_a)));

                    // May want to handle gracefully when note gets in/out of visible pitch range.
                    // Just patching with existing y for now.
                    let y_a = key_ys.get(&p_a).or(key_ys.get(&p_b)).unwrap_or(&y);
                    let y_b = key_ys.get(&p_b).or(key_ys.get(&p_a)).unwrap_or(&y);
                    y = egui::lerp(*y_a..=*y_b, coeff);

                    t1 = egui::lerp(t1_a as f64..=t1_b as f64, coeff as f64) as i64;
                    t2 = egui::lerp(t2_a as f64..=t2_b as f64, coeff as f64) as i64;

                    let c_a = note_color(&v_a, is_selected);
                    let c_b = note_color(&v_b, is_selected);
                    color = Self::transition_color(c_a, c_b, coeff);
                };
            }
            self.draw_note(&painter, (t1, t2), y, *half_tone_step, color);
        }
    }

    pub fn show(&mut self, ui: &mut Ui) -> StaveResponse {
        self.transition = self
            .transition
            .take()
            .map(|tr| tr.update(&ui.ctx()))
            .filter(|tr| tr.value().is_some());
        let stave_response = self.view(ui);

        if let Some(note_id) = stave_response.note_hovered {
            if stave_response.response.clicked() {
                self.note_selection.toggle(&note_id);
            }
        }

        let inner = &stave_response.response;
        self.update_new_note_draw(
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
        time_hovered: &Option<Time>,
        event: &TrackEvent,
        pitch: &Pitch,
    ) -> bool {
        if let Some(t) = &time_hovered {
            if let Some(p) = pitch_hovered {
                event.is_active(*t) && p == pitch
            } else {
                false
            }
        } else {
            false
        }
    }

    const KEYBOARD_TIME_STEP: Time = 10_000;

    fn handle_commands(&mut self, response: &egui::Response) -> Option<Time> {
        // TODO Have to see if duplication here can be reduced. Likely the dispatch needs some
        //   hash map that for each input state defines a unique command.
        //   Need to support focus somehow so the commands only active when stave is focused.
        //   Currently commands also affect other widgets (e.g. arrows change button focus).

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
            if let Some(time_selection) = &self.time_selection.clone() {
                self.do_edit_command(&response.ctx, response.id, |stave, track| {
                    tape_delete(track, &(time_selection.start, time_selection.end))
                });
            }
            if !self.note_selection.selected.is_empty() {
                self.do_edit_command(&response.ctx, response.id, |stave, track| {
                    // Deleting both time and event selection in one command for convenience, these can be separate.
                    delete_selected(track, &stave.note_selection.selected)
                });
            }
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::NONE,
                egui::Key::Insert,
            ))
        }) {
            if let Some(time_selection) = &self.time_selection.clone() {
                self.do_edit_command(&response.ctx, response.id, |stave, track| {
                    tape_insert(&(time_selection.start, time_selection.end))
                });
            }
        }

        // Tail shift
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL | Modifiers::SHIFT,
                egui::Key::ArrowRight,
            ))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                shift_tail(track, &(stave.cursor_position), &Stave::KEYBOARD_TIME_STEP)
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL | Modifiers::SHIFT,
                egui::Key::ArrowLeft,
            ))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                shift_tail(track, &(stave.cursor_position), &-Stave::KEYBOARD_TIME_STEP)
            });
        }

        // Note time moves
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::ALT | Modifiers::SHIFT,
                egui::Key::ArrowRight,
            )) || i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::SHIFT, egui::Key::L))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                shift_selected(
                    track,
                    &stave.note_selection.selected,
                    &Stave::KEYBOARD_TIME_STEP,
                )
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::ALT | Modifiers::SHIFT,
                egui::Key::ArrowLeft,
            )) || i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::SHIFT, egui::Key::H))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                shift_selected(
                    track,
                    &stave.note_selection.selected,
                    &-Stave::KEYBOARD_TIME_STEP,
                )
            });
        }

        // Note edits
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::H))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                stretch_selected_notes(
                    track,
                    &stave.note_selection.selected,
                    &-Stave::KEYBOARD_TIME_STEP,
                )
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::L))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                stretch_selected_notes(
                    track,
                    &stave.note_selection.selected,
                    &Stave::KEYBOARD_TIME_STEP,
                )
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::U))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                transpose_selected_notes(track, &stave.note_selection.selected, 1)
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::J))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                transpose_selected_notes(track, &stave.note_selection.selected, -1)
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::I))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                accent_selected_notes(track, &stave.note_selection.selected, 1)
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::K))
        }) {
            self.do_edit_command(&response.ctx, response.id, |stave, track| {
                accent_selected_notes(track, &stave.note_selection.selected, -1)
            });
        }

        // Undo/redo
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::CTRL, egui::Key::Z))
        }) {
            let mut changes = vec![];
            let edit_state = if self.history.borrow_mut().undo(&mut changes) {
                Some((EditCommandId::Undo, changes))
            } else {
                None
            };
            self.transition = Self::animate_edit(&response.ctx, response.id, edit_state);
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::CTRL, egui::Key::Y))
                || i.consume_shortcut(&egui::KeyboardShortcut::new(
                    Modifiers::CTRL | Modifiers::SHIFT,
                    egui::Key::Z,
                ))
        }) {
            let mut changes = vec![];
            let edit_state = if self.history.borrow_mut().redo(&mut changes) {
                Some((EditCommandId::Redo, changes))
            } else {
                None
            };
            self.transition = Self::animate_edit(&response.ctx, response.id, edit_state);
        }

        // Bookmarks & time navigation
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::M))
        }) {
            self.bookmarks.set(self.cursor_position);
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::N))
        }) {
            self.bookmarks.remove(&self.cursor_position);
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL,
                egui::Key::ArrowLeft,
            ))
        }) {
            return self.bookmarks.previous(&self.cursor_position).or(Some(0));
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL,
                egui::Key::ArrowRight,
            ))
        }) {
            return self
                .bookmarks
                .next(&self.cursor_position)
                .or(Some(self.max_time()));
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
            return Some(self.max_time());
        }
        if let Some(hover_pos) = response.hover_pos() {
            if response.middle_clicked() {
                let at = self.time_from_x(hover_pos.x);
                return Some(at);
            }
        }

        None
    }

    fn animate_edit(
        context: &Context,
        transition_id: egui::Id,
        edit_state: Option<(EditCommandId, EventActionsList)>,
    ) -> Option<EditTransition> {
        if let Some((command_id, changes)) = edit_state {
            let mut changeset = Changeset::empty();
            changeset.add_all(&changes);
            Some(EditTransition::start(
                context,
                transition_id,
                command_id,
                changeset,
            ))
        } else {
            None
        }
    }

    fn do_edit_command<Action: FnOnce(&Stave, &Track) -> Option<AppliedCommand>>(
        &mut self,
        context: &Context,
        transition_id: egui::Id,
        action: Action,
    ) {
        self.transition = Self::animate_edit(
            context,
            transition_id,
            self.history
                .borrow_mut()
                .update_track(|track| action(&self, track)),
        )
    }

    fn max_time(&self) -> Time {
        self.history.borrow().with_track(|track| track.max_time())
    }

    fn update_time_selection(&mut self, response: &egui::Response, time: &Option<Time>) {
        let drag_button = PointerButton::Primary;
        if response.clicked_by(drag_button) {
            self.time_selection = None;
        } else if response.drag_started_by(drag_button) {
            if let Some(time) = time {
                self.time_selection = Some(*time..*time);
            }
        } else if response.drag_released_by(drag_button) {
            // Just documenting how it can be handled
        } else if response.dragged_by(drag_button) {
            if let Some(time) = time {
                if let Some(selection) = &mut self.time_selection {
                    selection.end = *time;
                }
            }
        }
    }

    fn update_new_note_draw(
        &mut self,
        response: &egui::Response,
        modifiers: &Modifiers,
        time: &Option<Time>,
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
                        time: *time..*time,
                        pitch: *pitch,
                    });
                }
            }
        } else if response.drag_released_by(drag_button) {
            if let Some(draw) = &self.note_draw.clone() {
                if !draw.time.is_empty() {
                    let time_range = (draw.time.start, draw.time.end);
                    let id_seq = &self.history.borrow().id_seq.clone();
                    self.do_edit_command(&response.ctx, response.id, |_stave, track| {
                        if draw.pitch == PIANO_DAMPER_LINE {
                            if modifiers.alt {
                                set_damper(id_seq, track, &time_range, false)
                            } else {
                                set_damper(id_seq, track, &time_range, true)
                            }
                        } else {
                            add_new_note(id_seq, &time_range, &draw.pitch)
                        }
                    });
                }
            }
            self.note_draw = None;
        } else if response.dragged_by(drag_button) {
            if let Some(time) = time {
                if let Some(draw) = &mut self.note_draw {
                    draw.time.end = *time;
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
        time_range: (Time, Time),
        y: Pix,
        height: Pix,
        color: Color32,
    ) {
        let paint_rect = Rect {
            min: Pos2 {
                x: self.x_from_time(time_range.0),
                y: y - height * 0.45,
            },
            max: Pos2 {
                x: self.x_from_time(time_range.1),
                y: y + height * 0.45,
            },
        };
        painter.rect_filled(paint_rect, Rounding::ZERO, color);
    }

    fn default_draw_note(
        &self,
        painter: &Painter,
        velocity: Level,
        x_range: (Time, Time),
        y: Pix,
        height: Pix,
        selected: bool,
    ) {
        self.draw_note(painter, x_range, y, height, note_color(&velocity, selected));
    }

    fn transition_color(color_a: Color32, color_b: Color32, coeff: f32) -> Color32 {
        // color a -> red -> color b
        if coeff < 0.5 {
            egui::lerp(Rgba::from(color_a)..=Rgba::from(Color32::RED), 2.0 * coeff).into()
        } else {
            egui::lerp(
                Rgba::from(Color32::RED)..=Rgba::from(color_b),
                2.0 * f32::abs(coeff - 0.5),
            )
            .into()
        }
    }

    fn cc_animation_params(ev: Option<&TrackEvent>) -> Option<(Time, Level)> {
        ev.and_then(|ev| {
            if let TrackEventType::Controller(cc) = &ev.event {
                debug_assert_eq!(cc.controller_id, MIDI_CC_SUSTAIN_ID);
                Some((ev.at, cc.value))
            } else {
                None
            }
        })
    }

    fn draw_track_cc(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        painter: &Painter,
        should_be_visible: &mut Option<util::Range<Time>>,
        last_damper_value: &mut (Time, Level),
        event: &TrackEvent,
        cc: &ControllerSetValue,
    ) {
        if cc.controller_id == MIDI_CC_SUSTAIN_ID {
            if let Some(y) = key_ys.get(&PIANO_DAMPER_LINE) {
                let mut color = note_color(&cc.value, false);
                let mut t = event.at;
                if let Some(trans) = &self.transition {
                    let coeff = trans.value().unwrap();
                    if let Some(change) = trans.changeset.changes.get(&event.id) {
                        let (t1, v1) =
                            Self::cc_animation_params(change.before()).unwrap_or((event.at, 0));
                        let (t2, v2) =
                            Self::cc_animation_params(change.after()).unwrap_or((event.at, 0));

                        t = egui::lerp(t1 as f64..=t2 as f64, coeff as f64) as i64;

                        let c_a = note_color(&v1, false);
                        let c_b = note_color(&v2, false);
                        color = Self::transition_color(c_a, c_b, coeff);
                        *should_be_visible = should_be_visible
                            .map(|r| (r.0.min(last_damper_value.0), r.1.max(t2)))
                            .or(Some((last_damper_value.0, t2)));
                        debug_assert!(should_be_visible.is_some());
                    }
                }
                // TODO (improvement) The time range here is not right: is shown up to the event,
                //   should be from event to the next one instead.
                self.draw_note(
                    painter,
                    (last_damper_value.0, t),
                    *y,
                    *half_tone_step,
                    color,
                );
                *last_damper_value = (event.at, cc.value);
            }
        }
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

    pub fn draw_time_selection(&self, painter: &Painter, selection: &Range<Time>, color: &Color32) {
        let clip = painter.clip_rect();
        let area = Rect {
            min: Pos2 {
                x: self.x_from_time(selection.start),
                y: clip.min.y,
            },
            max: Pos2 {
                x: self.x_from_time(selection.end),
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

    fn ensure_visible(&mut self, at: Time) {
        let x_range = self.view_rect.x_range();
        let x = self.x_from_time(at);
        if !x_range.contains(x) {
            if x_range.max < x {
                self.scroll_to(at, 0.7);
            } else {
                self.scroll_to(at, 0.3);
            }
        }
    }

    fn is_visible(&self, at: Time) -> bool {
        self.view_rect.x_range().contains(self.x_from_time(at))
    }
}

fn draw_selection_hints(
    painter: &Painter,
    key_ys: &BTreeMap<Pitch, Pix>,
    half_tone_step: &Pix,
    x: f32,
    pitches: &HashSet<Pitch>,
) {
    for p in pitches {
        if let Some(y) = key_ys.get(p) {
            painter.circle_filled(Pos2::new(x, *y), *half_tone_step, COLOR_SELECTED);
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
    if selected {
        COLOR_SELECTED.into()
    } else {
        egui::lerp(
            Rgba::from_rgb(0.6, 0.7, 0.7)..=Rgba::from_rgb(0.0, 0.0, 0.0),
            *velocity as f32 / MAX_LEVEL as f32,
        )
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_serialization() {
        let file_path = PathBuf::from("./target/test_bookmarks_serialization");

        let mut bookmarks = Bookmarks::new(&file_path);
        bookmarks.set(12);
        bookmarks.set(23);
        bookmarks.store_to(&file_path);

        let mut bookmarks = Bookmarks::new(&file_path);
        bookmarks.load_from(&file_path);
        assert_eq!(bookmarks.list.len(), 2);
        assert_eq!(bookmarks.iter().count(), 2);
        assert!(bookmarks.list.contains(&Bookmark { at: 12 }));
        assert!(bookmarks.list.contains(&Bookmark { at: 23 }));
    }
}

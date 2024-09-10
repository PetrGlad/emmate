use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
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
use crate::ev::{ControllerId, Level, Pitch, Velocity};
use crate::track::{export_smf, EventId, Track, MAX_LEVEL, MIDI_CC_SUSTAIN_ID};
use crate::track_edit::{
    accent_selected_notes, add_new_note, clear_bookmark, delete_selected, set_bookmark, set_damper,
    shift_selected, shift_tail, stretch_selected_notes, tape_delete, tape_insert,
    transpose_selected_notes, AppliedCommand, EditCommandId,
};
use crate::track_history::{CommandApplication, TrackHistory};
use crate::{ev, util, Pix};

// Tone 60 is C3, tones start at C-2 (tone 21).
const PIANO_LOWEST_KEY: Pitch = 21;
const PIANO_KEY_COUNT: Pitch = 88;
/// Reserve this ley lane for damper display.
const PIANO_DAMPER_LANE: Pitch = PIANO_LOWEST_KEY - 1;
pub(crate) const PIANO_KEY_LINES: Range<Pitch> =
    PIANO_LOWEST_KEY..(PIANO_LOWEST_KEY + PIANO_KEY_COUNT);
/// All lanes including CC placeholder.
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

/// Note’s pitch is determined by the containing lane.
#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
enum LaneNote {
    On(Velocity),
    Off,
}

/// Controller's id is determined by the containing lane.
struct LaneCc(Velocity);

#[derive(Default)]
struct LaneBookmark();

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct LaneEvent<Ev> {
    pub id: EventId,
    pub at: Time,
    pub ev: Ev,
}

/// In contrast to Track this contain only one type of events, which should streamline
/// calculations and help compiler to optimize the code.
#[derive(Debug, Default, Clone)]
pub struct Lane<Ev> {
    pub events: Vec<LaneEvent<Ev>>,
}

impl<Ev> Lane<Ev> {
    pub fn max_time(&self) -> Option<Time> {
        self.events.last().map(|ev| ev.at)
    }
}

pub type LaneIndex = i8;

// TODO Is this helpful or just keep Vecs instead?
#[derive(Debug, Default, Clone)]
pub struct Lanes<Ev>(Vec<Lane<Ev>>);

impl<Ev> Lanes<Ev> {
    pub fn new() -> Self {
        Lanes(vec![])
    }
}

#[derive(Default)]
/// View model of a track.
/// 88 lanes for notes, 1 lane for damper, 1 lane for bookmarks
struct TrackLanes {
    pub notes: Vec<Lane<ev::Tone>>,
    pub cc: Vec<Lane<ev::Cc>>,
    pub bookmarks: Vec<Lane<ev::Bookmark>>,
}

impl TrackLanes {
    fn new(track: &Track) -> Self {
        let mut lanes = TrackLanes::default();
        lanes.update(track);
        lanes
    }

    fn reset(&mut self) {
        self.notes.clear();
        self.notes.resize(Pitch::MAX as usize + 1, Lane::default());
        self.cc.clear();
        self.cc
            .resize(ControllerId::MAX as usize + 1, Lane::default());
        // Still using array for bookmarks to keep the code consistent.
        self.bookmarks.clear();
        self.bookmarks.resize(1, Lane::default());
    }

    fn update(&mut self, track: &Track) {
        self.reset();
        for ev in &track.items {
            match &ev.ev {
                ev::Type::Note(n) => self.notes[n.pitch as usize].events.push(LaneEvent {
                    id: ev.id,
                    at: ev.at,
                    ev: n.clone(),
                }),
                ev::Type::Cc(cc) => self.cc[cc.controller_id as usize].events.push(LaneEvent {
                    id: ev.id,
                    at: ev.at,
                    ev: cc.clone(),
                }),
                ev::Type::Bookmark(bm) => self.bookmarks[0].events.push(LaneEvent {
                    id: ev.id,
                    at: ev.at,
                    ev: bm.clone(),
                }),
            }
        }
    }
}

// UI state representing a currently drawn note,
#[derive(Debug, Clone)]
struct NoteDraw {
    time: Range<Time>,
    pitch: Pitch,
}

#[derive(Debug, Default)]
pub struct NotesSelection {
    /// Starting events of selectable item ranges.
    selected: HashSet<EventId>,
}

impl NotesSelection {
    fn toggle(&mut self, id: &EventId) {
        // FIXME select/deselect related events OR update edit actions to also affect related events.
        if self.selected.contains(&id) {
            self.selected.remove(&id);
        } else {
            self.selected.insert(*id);
        }
    }

    fn contains(&self, ev: &LaneEvent<ev::Tone>) -> bool {
        self.selected.contains(&ev.id)
    }

    fn clear(&mut self) {
        self.selected.clear();
    }

    pub fn count(&self) -> usize {
        self.selected.len()
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
        self.coeff = ctx.animate_bool(self.animation_id, true);
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

pub struct Stave {
    /// The track's reference data.
    pub history: RefCell<TrackHistory>,
    /// View model of the track.
    lanes: TrackLanes,

    /// Starting moment of visible time range.
    pub time_left: Time,
    /// End moment of visible time range.
    pub time_right: Time,
    /// The widget's displayed rectangle coordinates.
    pub view_rect: Rect,

    pub cursor_position: Time,
    pub time_selection: Option<Range<Time>>,
    /// Currently drawn note.
    note_draw: Option<NoteDraw>,
    pub note_selection: NotesSelection,
    /// Change animation parameters.
    pub transition: Option<EditTransition>,
}

const COLOR_SELECTED: Rgba = Rgba::from_rgb(0.7, 0.1, 0.3);
const COLOR_HOVERED: Rgba = Rgba::from_rgb(0.1, 0.4, 1.0);

struct InnerResponse {
    response: egui::Response,
    pitch_hovered: Option<Pitch>,
    time_hovered: Option<Time>,
    note_hovered: Vec<EventId>,
    modifiers: Modifiers,
}

pub struct StaveResponse {
    pub ui_response: egui::Response,
    pub new_cursor_position: Option<Time>,
}

impl Stave {
    /// Limit viewable range to +-30 hours to avoid under/overflows and stay in a sensible range.
    /// World record playing piano seems to be 130 hours so some might find this limiting.
    // I would like to use Duration but that is not 'const' yet.
    const ZOOM_TIME_LIMIT: Time = 30 * 60 * 60 * 1_000_000;

    pub fn new(history: RefCell<TrackHistory>) -> Stave {
        let lanes = TrackLanes::new(history.borrow().track.read().as_ref());
        Stave {
            history,
            lanes,
            time_left: 0,
            time_right: chrono::Duration::minutes(5).num_microseconds().unwrap(),
            view_rect: Rect::NOTHING,
            cursor_position: 0,
            time_selection: None,
            note_draw: None,
            note_selection: NotesSelection::default(),
            transition: None,
        }
    }

    pub fn save_to(&mut self, file_path: &PathBuf) {
        self.history
            .borrow()
            .with_track(|track| export_smf(&track.items, file_path));
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
        // Zoom while attempting to keep the time position under mouse pointer.
        // Normally it will stay put but shift when we reach hard limits.
        // TODO (cleanup) Consider using emath::remap
        let at = self.time_from_x(mouse_x);
        self.time_left = (at - ((at - self.time_left) as f32 / zoom_factor) as Time)
            .max(-Self::ZOOM_TIME_LIMIT)
            - 1;
        self.time_right = (at + ((self.time_right - at) as f32 / zoom_factor) as Time)
            .min(Self::ZOOM_TIME_LIMIT)
            + 1;
        assert!(self.time_left < self.time_right)
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
                let mut note_hovered = vec![];
                let should_be_visible = self.draw_events(
                    &key_ys,
                    &half_tone_step,
                    &pointer_pos,
                    &mut note_hovered,
                    &painter,
                );
                self.draw_cursor(
                    &painter,
                    self.x_from_time(self.cursor_position),
                    Rgba::from_rgba_unmultiplied(0.0, 0.5, 0.0, 0.7).into(),
                );

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

    fn draw_events(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        pointer_pos: &Option<Pos2>,
        note_hovered: &mut Vec<EventId>,
        painter: &Painter,
    ) -> Option<util::Range<Time>> {
        let x_range = painter.clip_rect().x_range();
        // Selection hints that remind of selected notes that are not currently visible.
        let mut selection_hints_left: HashSet<Pitch> = HashSet::new();
        let mut selection_hints_right: HashSet<Pitch> = HashSet::new();
        // Contains time range that includes all events affected by an edit action.
        let mut should_be_visible = None;
        let is_in_transition = |id| {
            if let Some(trans) = &self.transition {
                trans.changeset.changes.contains_key(id)
            } else {
                false
            }
        };

        // Paint notes
        // The notes may intersect in some cases.
        for (pitch, lane) in self.lanes.notes.iter().enumerate() {
            if let Some(y) = key_ys.get(&(pitch as Pitch)) {
                let mut iev = lane.events.iter();
                while let Some(note) = iev.next() {
                    assert_eq!(note.ev.pitch, pitch as Pitch);
                    /////////// dbg!(note);
                    if !note.ev.on || is_in_transition(&note.id) {
                        continue;
                    }
                    debug_assert!(note.ev.end.is_some());
                    // O(N^2) but end should not be too far away, in most cases it is just next.
                    let end = note
                        .ev
                        .end
                        .and_then(|end_id| iev.clone().find(|x| x.id == end_id));
                    ///// debug_assert!(end.map(|x| !x.ev.on).unwrap_or(true));
                    let note_rect = self.draw_track_note(y, half_tone_step, painter, note, end);
                    if let Some(pointer_pos) = pointer_pos {
                        if let Some(r) = note_rect {
                            if r.contains(*pointer_pos) {
                                painter.rect_stroke(
                                    r,
                                    Rounding::ZERO,
                                    Stroke::new(2.0, COLOR_HOVERED),
                                );
                                note_hovered.push(note.id);
                                if let Some(end) = end {
                                    note_hovered.push(end.id);
                                }
                            }
                        }
                    }
                    if self.note_selection.contains(&note) {
                        if x_range.max < self.x_from_time(note.at) {
                            selection_hints_right.insert(note.ev.pitch);
                        } else if let Some(ev) = end {
                            if self.x_from_time(ev.at) < x_range.min {
                                selection_hints_left.insert(ev.ev.pitch);
                            }
                        }
                    }
                }
            }
        }

        {
            // Paint sustain lane
            let mut last_damper = (0 as Time, 0 as Level);
            for cc in &self.lanes.cc[MIDI_CC_SUSTAIN_ID as usize].events {
                assert_eq!(cc.ev.controller_id, MIDI_CC_SUSTAIN_ID);
                self.draw_track_cc(&key_ys, half_tone_step, &painter, &last_damper, &cc);
                last_damper = (cc.at, cc.ev.value);
            }
        }

        for bm in &self.lanes.bookmarks[0].events {
            self.draw_cursor(
                &painter,
                self.x_from_time(bm.at),
                Rgba::from_rgba_unmultiplied(0.0, 0.4, 0.0, 0.3).into(),
            );
        }

        if false {
            // FIXME (implementation) Update transition animations.
            if let Some(trans) = &self.transition {
                for (_ev_id, action) in &trans.changeset.changes {
                    // TODO (cleanup) Explicitly restrict actions to not change event types,
                    //      this should reduce number of cases to consider here.

                    let note_a = Stave::note_animation_params(action.before());
                    let note_b = Stave::note_animation_params(action.after());
                    if note_a.is_some() || note_b.is_some() {
                        self.draw_note_transition(
                            key_ys,
                            half_tone_step,
                            painter,
                            &mut should_be_visible,
                            trans.coeff,
                            false,
                            note_a,
                            note_b,
                        );
                    }

                    let cc_a = Stave::cc_animation_params(action.before());
                    let cc_b = Stave::cc_animation_params(action.after());
                    if cc_a.is_some() || cc_b.is_some() {
                        self.draw_cc_transition(
                            key_ys,
                            half_tone_step,
                            painter,
                            &mut should_be_visible,
                            trans.coeff,
                            cc_a,
                            cc_b,
                        );
                    }

                    if !(note_a.is_some() || note_b.is_some() || cc_a.is_some() || cc_b.is_some()) {
                        // TODO (implementation, ux) Handle bookmarks (can be either animated somehow or just ignored).
                        print!("WARN No animation params (a bookmark?).");
                    }
                }
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

    pub fn show(&mut self, ui: &mut Ui) -> StaveResponse {
        self.transition = self
            .transition
            .take()
            .map(|tr| tr.update(&ui.ctx()))
            .filter(|tr| tr.value().is_some());
        if self.transition.is_none() {
            ui.ctx().clear_animations();
        }
        let stave_response = self.view(ui);

        if stave_response.response.clicked() {
            if !ui.input(|i| i.modifiers.ctrl) {
                self.note_selection.clear()
            }
            for event_id in stave_response.note_hovered {
                self.note_selection.toggle(&event_id);
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

    const KEYBOARD_TIME_STEP: Time = 10_000;

    /**
     * Applies the command and returns time to move the stave cursor to.
     */
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
                self.do_edit_command(&response.ctx, response.id, |_stave, track| {
                    tape_delete(track, &(time_selection.start, time_selection.end))
                });
            }
            if !self.note_selection.selected.is_empty() {
                self.do_edit_command(&response.ctx, response.id, |stave, track| {
                    // Deleting both time and event selection in one command for convenience, these can be separate commands.
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
                self.do_edit_command(&response.ctx, response.id, |_stave, _track| {
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
            let at = self.cursor_position;
            let id_seq = &self.history.borrow().id_seq.clone();
            self.do_edit_command(&response.ctx, response.id, |_stave, track| {
                set_bookmark(track, id_seq, &at)
            });
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::N))
        }) {
            let at = self.cursor_position;
            self.do_edit_command(&response.ctx, response.id, |_stave, track| {
                clear_bookmark(track, &at)
            });
        }
        // Previous bookmark
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL,
                egui::Key::ArrowLeft,
            ))
        }) {
            let at = self.cursor_position;
            return self
                .history
                .borrow()
                .with_track(|track| {
                    track
                        .items
                        .iter()
                        .rfind(|ev| ev.at < at && matches!(ev.ev, ev::Type::Bookmark(_)))
                        .cloned()
                })
                .map(|ev| ev.at)
                .or(Some(0));
        }
        // Next bookmark
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::CTRL,
                egui::Key::ArrowRight,
            ))
        }) {
            let at = self.cursor_position;
            return self
                .history
                .borrow()
                .with_track(move |track| {
                    track
                        .items
                        .iter()
                        .find(|ev| ev.at > at && matches!(ev.ev, ev::Type::Bookmark(_)))
                        .cloned()
                })
                .map(|ev| ev.at)
                .or(Some(self.max_time()));
        }
        // Previous note/event
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::ALT,
                egui::Key::ArrowLeft,
            ))
        }) {
            let at = self.cursor_position;
            return self
                .history
                .borrow()
                .with_track(|track| track.items.iter().rfind(|ev| ev.at < at).cloned())
                .map(|ev| ev.at)
                .or(Some(0));
        }
        // Next note/event
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::ALT,
                egui::Key::ArrowRight,
            ))
        }) {
            let at = self.cursor_position;
            return self
                .history
                .borrow()
                .with_track(move |track| track.items.iter().find(|ev| ev.at > at).cloned())
                .map(|ev| ev.at)
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
        diff: Option<(EditCommandId, EventActionsList)>,
    ) -> Option<EditTransition> {
        if let Some((command_id, changes)) = diff {
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
    ) -> CommandApplication {
        let diff = self
            .history
            .borrow_mut()
            .update_track(|track| action(&self, track));
        // TODO (refactoring) No strict need to access the track through history anymore,
        // but we still need to keep  all the parts updated. Are the alternatives
        // (using channel pub/sub, maybe) better?
        self.lanes
            .update(self.history.borrow().track.read().as_ref());
        self.transition = Self::animate_edit(
            context,
            transition_id,
            diff.clone().map(|diff| (diff.0 .0, diff.1)),
        );
        diff
    }

    fn max_time(&self) -> Time {
        self.history
            .borrow()
            .with_track(|track| track.max_time())
            .unwrap_or(0)
    }

    fn update_time_selection(&mut self, response: &egui::Response, time: &Option<Time>) {
        let drag_button = PointerButton::Primary;
        if response.clicked_by(drag_button) {
            self.time_selection = None;
        } else if response.drag_started_by(drag_button) {
            if let Some(time) = time {
                self.time_selection = Some(*time..*time);
            }
        } else if response.drag_stopped_by(drag_button) {
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
        } else if response.drag_stopped_by(drag_button) {
            if let Some(draw) = &self.note_draw.clone() {
                if !draw.time.is_empty() {
                    let time_range = (draw.time.start, draw.time.end);
                    let id_seq = &self.history.borrow().id_seq.clone();
                    self.do_edit_command(&response.ctx, response.id, |_stave, track| {
                        if draw.pitch == PIANO_DAMPER_LANE {
                            set_damper(id_seq, track, &time_range, !modifiers.alt)
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
        );
    }

    /// Extract a number of scalar values to interpolate from a note event.
    fn note_animation_params(ev: Option<&ev::Item>) -> Option<((Time, Time), Pitch, Level)> {
        ev.and_then(|ev| {
            if let ev::Type::Note(n) = &ev.ev {
                todo!("Use on/off event pairs for duration.");
                Some(((ev.at, ev.at /*+ n.duration*/), n.pitch, n.velocity))
            } else {
                None // CC is animated separately.
            }
        })
    }

    fn draw_track_note(
        &self,
        y: &Pix,
        half_tone_step: &Pix,
        painter: &Painter,
        this: &LaneEvent<ev::Tone>,
        next: Option<&LaneEvent<ev::Tone>>,
    ) -> Option<Rect> {
        if !this.ev.on {
            eprintln!("Unmatched note event {:?} (next {:?})", this, next);
            return None;
        }
        let end_at = next.map(|ev| ev.at).unwrap_or(Time::MAX);
        Some(self.draw_note(
            &painter,
            (this.at, end_at),
            *y,
            *half_tone_step,
            note_color(&this.ev.velocity, self.note_selection.contains(&this)),
        ))
    }

    fn draw_note_transition(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        painter: &Painter,
        should_be_visible: &mut Option<util::Range<Time>>,
        coeff: f32,
        is_selected: bool,
        a: Option<((Time, Time), Pitch, Level)>,
        b: Option<((Time, Time), Pitch, Level)>,
    ) {
        // Interpolate the note states.
        assert!(a.is_some() || b.is_some());
        let ((t1_a, t2_a), p_a, v_a) = a.or(b).unwrap();
        let ((t1_b, t2_b), p_b, v_b) = b.or(a).unwrap();

        *should_be_visible = should_be_visible
            .map(|(a, b)| (a.min(t1_a), b.max(t2_a)))
            .or(Some((t1_a, t2_a)));

        // May want to handle gracefully when note gets in/out of visible pitch range.
        // Just patching with existing y for now.
        let y_a = key_ys.get(&p_a).or(key_ys.get(&p_b)).unwrap();
        let y_b = key_ys.get(&p_b).or(key_ys.get(&p_a)).unwrap();
        let y = egui::lerp(*y_a..=*y_b, coeff);

        let t1 = egui::lerp(t1_a as f64..=t1_b as f64, coeff as f64) as i64;
        let t2 = egui::lerp(t2_a as f64..=t2_b as f64, coeff as f64) as i64;

        let c_a = note_color(&v_a, is_selected);
        let c_b = note_color(&v_b, is_selected);
        let color = Self::transition_color(c_a, c_b, coeff);

        self.draw_note(&painter, (t1, t2), y, *half_tone_step, color);
    }

    fn draw_note(
        &self,
        painter: &Painter,
        time_range: (Time, Time),
        y: Pix,
        height: Pix,
        color: Color32,
    ) -> Rect {
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
        paint_rect
    }

    fn draw_point_accent(
        &self,
        painter: &Painter,
        time: Time,
        y: Pix,
        height: Pix,
        color: Color32,
    ) {
        painter.circle_filled(
            Pos2 {
                x: self.x_from_time(time),
                y,
            },
            height / 2.2,
            color,
        );
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

    fn cc_animation_params(ev: Option<&ev::Item>) -> Option<(Time, Level)> {
        ev.and_then(|ev| {
            if let ev::Type::Cc(cc) = &ev.ev {
                debug_assert_eq!(cc.controller_id, MIDI_CC_SUSTAIN_ID);
                Some((ev.at, cc.value))
            } else {
                None
            }
        })
    }

    fn draw_cc_transition(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        painter: &Painter,
        should_be_visible: &mut Option<util::Range<Time>>,
        coeff: f32,
        a: Option<(Time, Level)>,
        b: Option<(Time, Level)>,
    ) {
        assert!(a.is_some() || b.is_some());
        if let Some(y) = key_ys.get(&PIANO_DAMPER_LANE) {
            let (t1, v1) = a.or(b).unwrap();
            let (t2, v2) = b.or(a).unwrap();

            let t = egui::lerp(t1 as f64..=t2 as f64, coeff as f64) as i64;

            let c_a = note_color(&v1, false);
            let c_b = note_color(&v2, false);
            let color = Self::transition_color(c_a, c_b, coeff);
            *should_be_visible = should_be_visible
                .map(|r| (r.0.min(t2), r.1.max(t2)))
                .or(Some((t2, t2)));
            debug_assert!(should_be_visible.is_some());
            // Previous CC value is not available here, so just showing an accent here for now.
            self.draw_point_accent(painter, t, *y, *half_tone_step, color);
        }
    }

    fn draw_track_cc(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        painter: &Painter,
        last_cc: &(Time, Level),
        cc: &LaneEvent<ev::Cc>,
    ) {
        if cc.ev.controller_id != MIDI_CC_SUSTAIN_ID {
            return;
        }
        if let Some(y) = key_ys.get(&PIANO_DAMPER_LANE) {
            self.draw_note(
                painter,
                (last_cc.0, cc.at),
                *y,
                *half_tone_step,
                note_color(&cc.ev.value, false),
            );
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
                color: color.gamma_multiply(0.5),
            },
        );
        painter.vline(
            area.max.x,
            clip.y_range(),
            Stroke {
                width: 1.0,
                color: color.gamma_multiply(0.5),
            },
        );
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
mod tests {}

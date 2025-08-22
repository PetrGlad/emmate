use crate::changeset::{Changeset, EventActionsList};
use crate::common::Time;
use crate::range::{Range, RangeLike, RangeSpan};
use crate::track::{
    export_smf, ControllerSetValue, EventId, Level, Note, Pitch, Track, TrackEvent, TrackEventType,
    MAX_LEVEL, MIDI_CC_SUSTAIN_ID,
};
use crate::track_edit::{
    accent_selected_notes, add_new_note, clear_bookmark, delete_selected, set_bookmark, set_damper,
    shift_selected, shift_tail, stretch_selected_notes, tape_delete, tape_insert, tape_stretch,
    transpose_selected_notes, AppliedCommand, EditCommandId,
};
use crate::track_history::{CommandApplication, TrackHistory};
use crate::{range, Pix};
use chrono::Duration;
use eframe::egui::TextStyle::Body;
use eframe::egui::{
    self, Align2, Color32, Context, CornerRadius, FontId, Frame, Margin, Modifiers, Painter,
    PointerButton, Pos2, Rangef, Rect, Response, Sense, Stroke, Ui,
};
use eframe::epaint::StrokeKind;
use egui::Rgba;
use ordered_float::OrderedFloat;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

// Tone 60 is C3, tones start at C-2 (tone 21).
const PIANO_LOWEST_KEY: Pitch = 21;
const PIANO_KEY_COUNT: Pitch = 88;
/// Reserve this ley lane for damper display.
const PIANO_DAMPER_LANE: Pitch = PIANO_LOWEST_KEY - 1;
pub(crate) const PIANO_KEY_LINES: Range<Pitch> =
    (PIANO_LOWEST_KEY, PIANO_LOWEST_KEY + PIANO_KEY_COUNT);
// Lines including controller values placeholder.
const STAVE_KEY_LINES: Range<Pitch> = (PIANO_LOWEST_KEY - 1, PIANO_LOWEST_KEY + PIANO_KEY_COUNT);

fn key_line_ys(view_y_range: &Rangef, pitches: Range<Pitch>) -> (BTreeMap<Pitch, Pix>, Pix) {
    let mut lines = BTreeMap::new();
    let step = view_y_range.span() / pitches.len() as Pix;
    let mut y = view_y_range.max - step / 2.0;
    for p in pitches.range() {
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

// #[derive(Debug)]
pub struct Stave {
    pub history: RefCell<TrackHistory>,

    /// Starting moment of visible time range.
    pub time_left: Time,
    /// End moment of visible time range.
    pub time_right: Time,
    /// The widget's displayed rectangle coordinates.
    pub view_rect: Rect,

    pub cursor_position: Time,
    pub time_selection: Option<Range<Time>>,
    /// Currently drawn note.
    pub note_draw: Option<NoteDraw>,
    pub note_selection: NotesSelection,
    /// Change animation parameters.
    pub transition: Option<EditTransition>,

    // Velocity -> note_color lookup map
    note_colors: Vec<Color32>,
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
    /// Limit viewable range to +-30 hours to avoid under/overflows and stay in a sensible range.
    /// World record playing piano seems to be 130 hours, so some might find this limiting.
    // I would like to use Duration but that is not "const compatible" yet.
    const ZOOM_TIME_LIMIT: Time = 30 * 60 * 60 * 1_000_000;

    pub fn new(history: RefCell<TrackHistory>) -> Stave {
        let mut note_colors = vec![];
        assert_eq!(Level::MIN, 0); // Otherwise need to adjust lookups.
        for velocity in Level::MIN..Level::MAX {
            note_colors.push(
                egui::lerp(
                    Rgba::from_rgb(0.6, 0.7, 0.7)..=Rgba::from_rgb(0.0, 0.0, 0.0),
                    velocity as f32 / MAX_LEVEL as f32,
                )
                .into(),
            );
        }

        Stave {
            history,
            time_left: 0,
            time_right: chrono::Duration::minutes(5).num_microseconds().unwrap(),
            view_rect: Rect::NOTHING,
            cursor_position: 0,
            time_selection: None,
            note_draw: None,
            note_selection: NotesSelection::default(),
            transition: None,
            note_colors,
        }
    }

    pub fn save_to(&mut self, file_path: &PathBuf) {
        self.history
            .borrow()
            .with_track(|track| export_smf(&track.events, file_path));
    }

    /// Pixel/uSec, can be cached.
    pub fn time_scale(&self) -> f32 {
        debug_assert!(self.view_rect.width() > 0.0);
        self.view_rect.width() / (self.time_right - self.time_left) as f32
    }

    pub fn x_from_time(&self, at: Time) -> Pix {
        debug_assert!(self.view_rect.width() > 0.0);
        self.view_rect.min.x + (at as f32 - self.time_left as f32) * self.time_scale()
    }

    pub fn time_from_x(&self, x: Pix) -> Time {
        debug_assert!(self.view_rect.width() > 0.0);
        self.time_left + ((x - self.view_rect.min.x) / self.time_scale()) as Time
    }

    pub fn zoom(&mut self, zoom_factor: f32, mouse_x: Pix) {
        // Zoom so that time position under mouse pointer stays put.
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

    pub fn zoom_to_fit(&mut self, time_margin: Time) {
        self.time_left = -time_margin;
        self.time_right = self.history.borrow().with_track(|tr| tr.max_time()) + time_margin;
    }

    pub fn scroll(&mut self, dt: Time) {
        if self.time_left + dt < -Self::ZOOM_TIME_LIMIT
            || self.time_right + dt > Self::ZOOM_TIME_LIMIT
        {
            return;
        }
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

    const NOTHING_ZONE: Range<Time> = (Time::MIN, 0);

    fn view(&mut self, ui: &mut Ui) -> InnerResponse {
        Frame::new()
            .inner_margin(Margin::symmetric(4.0 as i8, 4.0 as i8))
            .stroke(Stroke::NONE)
            .show(ui, |ui| {
                let mut bounds = ui.available_rect_before_wrap().clone();
                let egui_response = ui.allocate_response(bounds.size(), Sense::click_and_drag());

                {
                    // TODO (cleanup) Use stack layout instead?
                    let mut ruler_rect = bounds.clone();
                    let style = ui.ctx().style();
                    let ruler_height = style.text_styles[&Body].size;
                    *bounds.top_mut() += ruler_height;
                    self.view_rect = bounds;

                    ruler_rect.set_height(ruler_height);
                    // TODO (cleanup) Use painter_at instead.
                    self.draw_time_ruler(&ui.painter(), ruler_rect);
                }


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
                let should_be_visible;
                {
                    let history = self.history.borrow();
                    let track = history.track.read();
                    should_be_visible = self.draw_events(
                        &key_ys,
                        &half_tone_step,
                        &pointer_pos,
                        &mut note_hovered,
                        &painter,
                        &track,
                    );
                }
                self.draw_cursor(
                    &painter,
                    self.x_from_time(self.cursor_position),
                    Rgba::from_rgba_unmultiplied(0.0, 0.5, 0.0, 0.7).into(),
                );

                if let Some(new_note) = &self.note_draw {
                    self.default_draw_note(
                        &painter,
                        64,
                        (new_note.time.0, new_note.time.1),
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
                    note_hovered,
                    modifiers: ui.input(|i| i.modifiers),
                }
            })
            .inner
    }

    fn draw_time_ruler(&mut self, painter: &Painter, ruler_rect: Rect) {
        let tick_durations_s = [
            0.005,
            0.01,
            0.05,
            0.1,
            1.0,
            5.0,
            10.0,
            15.0,
            30.0,
            60.0,
            5.0 * 60.0,
            10.0 * 60.0,
            30.0 * 60.0,
            60.0f32 * 60.0,
            60.0f32 * 60.0 * 4.0,
            60.0f32 * 60.0 * 6.0,
        ];
        let time_width = ruler_rect.width() / self.time_scale();
        let tick_duration = tick_durations_s
            .iter()
            .find_map(|td| {
                let x = td * 1_000_000.0; // From seconds
                let nticks = time_width / x;
                if 2.0 < nticks && nticks < 20.0 {
                    Some(x)
                } else {
                    None
                }
            })
            .unwrap_or(time_width / 5.0)
            .round() as Time;
        assert!(tick_duration > 0);
        let start_tick = self.time_from_x(ruler_rect.min.x) / tick_duration;
        let end_tick = self.time_from_x(ruler_rect.max.x) / tick_duration;
        let mut last_x = self.x_from_time(-1);
        for tick in start_tick..end_tick + 1 {
            let at = tick * tick_duration;
            // Avoids labels overlapping.
            if last_x < self.x_from_time(at) {
                last_x = self.draw_time_tick(painter, ruler_rect, at).max.x;
            }
        }
    }

    fn split_time(t: Time) -> (u16, u16, u16, u16) {
        let t = t / 1_000;
        let (t, millis) = (t / 1000, t % 1000);
        let (t, seconds) = (t / 60, t % 60);
        let (hours, minutes) = (t / 60, t % 60);
        (hours as u16, minutes as u16, seconds as u16, millis as u16)
    }

    fn format_time(t: Time) -> String {
        let (hours, minutes, seconds, millis) = Self::split_time(t);
        let mut result: String = "".into();
        if hours > 0 {
            result.push_str(&format!("{}:", hours));
        }
        result.push_str(&format!("{}'{}", minutes, seconds));
        if millis > 0 {
            result.push_str(&format!(".{}", millis));
        }
        result
    }

    fn draw_time_tick(&mut self, painter: &Painter, ruler_rect: Rect, at: Time) -> Rect {
        let x = self.x_from_time(at);
        painter.rect_filled(
            Rect::from_x_y_ranges(
                Rangef::new(x, x + 1.0),
                Rangef::new(ruler_rect.min.y, ruler_rect.max.y),
            ),
            CornerRadius::from(1.0),
            Color32::GRAY,
        );

        painter.text(
            Pos2::new(x + 4.0, ruler_rect.min.y),
            Align2::LEFT_TOP,
            Self::format_time(at),
            FontId::proportional(14.0),
            Color32::DARK_GRAY,
        )
    }

    fn draw_events(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        pointer_pos: &Option<Pos2>,
        note_hovered: &mut Option<EventId>,
        painter: &Painter,
        track: &Track,
    ) -> Option<range::Range<Time>> {
        let mut last_damper_value: (Time, Level) = (0, 0);
        let x_range = painter.clip_rect().x_range();
        let mut selection_hints_left: HashSet<Pitch> = HashSet::new();
        let mut selection_hints_right: HashSet<Pitch> = HashSet::new();
        let mut should_be_visible = None;
        for i in 0..track.events.len() {
            let event = &track.events[i];
            if let Some(trans) = &self.transition {
                if trans.changeset.changes.contains_key(&event.id) {
                    continue;
                }
            }
            match &event.event {
                TrackEventType::Note(note) => {
                    if self.note_selection.contains(&event) {
                        if x_range.max < self.x_from_time(event.at) {
                            selection_hints_right.insert(note.pitch);
                        } else if self.x_from_time(event.at + note.duration) < x_range.min {
                            selection_hints_left.insert(note.pitch);
                        }
                    }
                    let note_rect =
                        self.draw_track_note(key_ys, half_tone_step, &painter, &event, &note);
                    // Alternatively, can return the known rect from draw_track_note above and check that.
                    if let Some(r) = note_rect {
                        if let Some(&pointer_pos) = pointer_pos.as_ref() {
                            if r.contains(pointer_pos) {
                                *note_hovered = Some(event.id);
                                painter.rect_stroke(
                                    r,
                                    CornerRadius::ZERO,
                                    Stroke::new(2.0, COLOR_HOVERED),
                                    StrokeKind::Inside,
                                );
                            }
                        }
                    }
                }
                TrackEventType::Controller(cc) => self.draw_track_cc(
                    &key_ys,
                    half_tone_step,
                    &painter,
                    &mut last_damper_value,
                    &event,
                    &cc,
                ),
                TrackEventType::Bookmark => self.draw_cursor(
                    &painter,
                    self.x_from_time(event.at),
                    Rgba::from_rgba_unmultiplied(0.0, 0.4, 0.0, 0.3).into(),
                ),
            }
        }
        if let Some(trans) = &self.transition {
            for (_ev_id, action) in &trans.changeset.changes {
                // TODO (cleanup) Restrict actions to not change event types,
                //      this should reduce number of cases to consider.

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
                    // TODO (implementation) Handle bookmarks (can be either animated somehow or just ignored).
                    log::trace!("No animation params (a bookmark?).");
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

        if let Some(note_id) = stave_response.note_hovered {
            if stave_response.response.clicked() {
                if !ui.input(|i| i.modifiers.ctrl) {
                    self.note_selection.clear()
                }
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
                return event.is_active_at(*t) && p == pitch;
            }
        }
        false
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

        // Tempo adjustment
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::SHIFT,
                egui::Key::CloseBracket,
            ))
        }) {
            if let Some(time_selection) = &self.time_selection.clone() {
                self.adjust_tempo(&response, &time_selection, 1.01);
            }
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::SHIFT,
                egui::Key::OpenBracket,
            ))
        }) {
            if let Some(time_selection) = &self.time_selection.clone() {
                self.adjust_tempo(&response, &time_selection, 1.0 / 1.01);
            }
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
                    tape_delete(track, &(time_selection.0, time_selection.1))
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
                    tape_insert(&(time_selection.0, time_selection.1))
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

        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::ALT, egui::Key::A))
        }) {
            self.zoom_to_fit(Duration::seconds(3).num_microseconds().unwrap_or_default());
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
                        .events
                        .iter()
                        .rfind(|ev| ev.at < at && ev.event == TrackEventType::Bookmark)
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
                        .events
                        .iter()
                        .find(|ev| ev.at > at && ev.event == TrackEventType::Bookmark)
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
                .with_track(|track| track.events.iter().rfind(|ev| ev.at < at).cloned())
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
                .with_track(move |track| track.events.iter().find(|ev| ev.at > at).cloned())
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

    fn adjust_tempo(&mut self, response: &Response, time_selection: &Range<Time>, ratio: f32) {
        if self
            .do_edit_command(&response.ctx, response.id, |_stave, track| {
                tape_stretch(track, &(time_selection.0, time_selection.1), ratio)
            })
            .is_some()
        {
            self.time_selection = self.time_selection.map(|r| {
                (
                    r.0,
                    r.1 + ((ratio - 1.0) * time_selection.len() as f32) as Time,
                )
            });
        }
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
        self.transition = Self::animate_edit(
            context,
            transition_id,
            diff.clone().map(|diff| (diff.0 .0, diff.1)),
        );
        diff
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
                self.time_selection = Some((*time, *time));
            }
        } else if response.drag_stopped_by(drag_button) {
            // Just documenting how it can be handled
        } else if response.dragged_by(drag_button) {
            if let Some(time) = time {
                if let Some(selection) = &mut self.time_selection {
                    selection.1 = *time;
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
                        time: (*time, *time),
                        pitch: *pitch,
                    });
                }
            }
        } else if response.drag_stopped_by(drag_button) {
            if let Some(draw) = &self.note_draw.clone() {
                if !draw.time.is_empty() {
                    let time_range = (draw.time.0, draw.time.1);
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
                    draw.time.1 = *time;
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

    fn note_animation_params(ev: Option<&TrackEvent>) -> Option<((Time, Time), Pitch, Level)> {
        ev.and_then(|ev| {
            if let TrackEventType::Note(n) = &ev.event {
                Some(((ev.at, ev.at + n.duration), n.pitch, n.velocity))
            } else {
                None // CC is animated separately.
            }
        })
    }

    fn note_color(&self, velocity: &Level, selected: bool) -> Color32 {
        if selected {
            COLOR_SELECTED.into()
        } else {
            self.note_colors[*velocity as usize]
        }
    }

    fn draw_track_note(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        painter: &Painter,
        event: &TrackEvent,
        note: &Note,
    ) -> Option<Rect> {
        if let Some(y) = key_ys.get(&note.pitch) {
            Some(self.draw_note(
                &painter,
                (event.at, event.at + note.duration),
                *y,
                *half_tone_step,
                self.note_color(&note.velocity, self.note_selection.contains(&event)),
            ))
        } else {
            None
        }
    }

    fn draw_note_transition(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        painter: &Painter,
        should_be_visible: &mut Option<range::Range<Time>>,
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

        let c_a = self.note_color(&v_a, is_selected);
        let c_b = self.note_color(&v_b, is_selected);
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
        painter.rect_filled(paint_rect, CornerRadius::ZERO, color);
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
        self.draw_note(
            painter,
            x_range,
            y,
            height,
            self.note_color(&velocity, selected),
        );
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

    fn draw_cc_transition(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        painter: &Painter,
        should_be_visible: &mut Option<range::Range<Time>>,
        coeff: f32,
        a: Option<(Time, Level)>,
        b: Option<(Time, Level)>,
    ) {
        assert!(a.is_some() || b.is_some());
        if let Some(y) = key_ys.get(&PIANO_DAMPER_LANE) {
            let (t1, v1) = a.or(b).unwrap();
            let (t2, v2) = b.or(a).unwrap();

            let t = egui::lerp(t1 as f64..=t2 as f64, coeff as f64) as i64;

            let c_a = self.note_color(&v1, false);
            let c_b = self.note_color(&v2, false);
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
        last_damper_value: &mut (Time, Level),
        event: &TrackEvent,
        cc: &ControllerSetValue,
    ) {
        if cc.controller_id == MIDI_CC_SUSTAIN_ID {
            if let Some(y) = key_ys.get(&PIANO_DAMPER_LANE) {
                // TODO (visuals, improvement) The time range here is not right: is shown up to the event,
                //   should be from the event to the next one instead.
                self.draw_note(
                    painter,
                    (last_damper_value.0, event.at),
                    *y,
                    *half_tone_step,
                    self.note_color(&cc.value, false),
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
                x: self.x_from_time(selection.0),
                y: clip.min.y,
            },
            max: Pos2 {
                x: self.x_from_time(selection.1),
                y: clip.max.y,
            },
        };
        painter.rect_filled(area, CornerRadius::ZERO, *color);
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

#[cfg(test)]
mod tests {}

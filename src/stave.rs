use crate::changeset::{Changeset, EventAction, EventActionsList};
use crate::common::{Time, VersionId};
use crate::range::{Range, RangeLike, RangeSpan};
use crate::track::{
    export_smf, ControllerSetValue, EventId, Level, Note, Pitch, Track,
    TrackEvent, TrackEventType, DEFAULT_CC_LEVEL, MAX_LEVEL, MIDI_CC_SUSTAIN_ID,
};
use crate::track_edit::{
    accent_selected_notes, add_new_note, clear_bookmark, delete_selected, set_bookmark,
    set_damper, shift_selected, shift_tail, stretch_selected_notes, tape_delete, tape_insert,
    tape_stretch, transpose_selected_notes, AppliedCommand, EditCommandId,
};
use crate::track_history::{CommandApplication, TrackHistory};
use crate::{range, Pix};
use chrono::Duration;
use eframe::egui::TextStyle::Body;
use eframe::egui::{
    self, Align2, Color32, Context, CornerRadius, FontId, Frame, Margin, Mesh, Modifiers, Painter,
    PointerButton, Pos2, Rangef, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use eframe::emath;
use eframe::epaint::{RectShape, TessellationOptions, Tessellator, Vertex};
use egui::Rgba;
use ordered_float::OrderedFloat;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

// Tone 60 is C3, tones start at C-2 (tone 21).
const PIANO_LOWEST_KEY: Pitch = 21;
const PIANO_KEY_COUNT: Pitch = 88;
/// Reserve this ley lane for damper display.
const CONTROL_LANES_COUNT: Pitch = 1;
const PIANO_DAMPER_LANE: Pitch = PIANO_LOWEST_KEY - 1;
pub(crate) const PIANO_KEY_LINES: Range<Pitch> =
    (PIANO_LOWEST_KEY, PIANO_LOWEST_KEY + PIANO_KEY_COUNT);
// Lanes including controller values placeholder.
const STAVE_KEY_LANES: Range<Pitch> = (
    PIANO_DAMPER_LANE,
    PIANO_DAMPER_LANE + PIANO_KEY_COUNT + CONTROL_LANES_COUNT,
);

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
    version: u64,
}

impl NotesSelection {
    fn toggle(&mut self, id: &EventId) {
        if self.selected.contains(&id) {
            self.selected.remove(&id);
        } else {
            self.selected.insert(*id);
        }
        self.version += 1;
    }

    fn contains(&self, ev_id: &EventId) -> bool {
        self.selected.contains(ev_id)
    }

    fn clear(&mut self) {
        self.selected.clear();
        self.version += 1;
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
    /// 0.0 -> 1.0
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

    pub fn update(&mut self, ctx: &Context) {
        self.coeff = ctx.animate_bool(self.animation_id, true);
    }

    pub fn value(&self) -> Option<f32> {
        if self.coeff >= 1.0 {
            None
        } else {
            Some(self.coeff)
        }
    }
}

// Selects viewable track range.
type TimeRange = Range<Time>;

#[derive(Clone, PartialEq)]
pub struct Viewport {
    /// Starting and ending moment of track's visible time range.
    time_range: TimeRange,
    /// The widget's displayed rectangle coordinates.
    view_rect: Rect,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            time_range: TimeRange::default(),
            view_rect: Rect::NOTHING,
        }
    }
}

// Zoom and scroll parameters.
impl Viewport {
    /// Limit viewable range to +-30 hours to avoid under/overflows and stay in a sensible range.
    /// World record playing piano seems to be 130 hours, so some might find this limiting.
    // I would like to use Duration but that is not "const compatible" yet.
    const ZOOM_TIME_LIMIT: Time = 30 * 60 * 60 * 1_000_000;

    pub fn lanes_y_half_tone(&self) -> f32 {
        self.view_rect.height() / STAVE_KEY_LANES.len() as f32
    }

    pub fn lanes_y_range(&self) -> Rangef {
        Rangef::new(
            self.view_rect.top() + self.lanes_y_half_tone() / 2.0,
            self.view_rect.bottom() - self.lanes_y_half_tone() / 2.0,
        )
    }

    /// Pixel/uSec, can be cached.
    #[inline]
    pub fn time_scale(&self) -> f32 {
        debug_assert!(self.view_rect.width() > 0.0);
        self.view_rect.width() / self.time_range.len() as f32
    }

    #[inline]
    pub fn x_from_time(&self, at: Time) -> Pix {
        debug_assert!(self.view_rect.width() > 0.0);
        self.view_rect.min.x + (at - self.time_range.0) as f32 * self.time_scale()
    }

    pub fn time_from_x(&self, x: Pix) -> Time {
        debug_assert!(self.view_rect.width() > 0.0);
        self.time_range.0 + ((x - self.view_rect.min.x) / self.time_scale()) as Time
    }

    // #[inline]
    // pub fn x_from_default(&self, x_default: Pix) -> Pix {
    //     debug_assert!(self.view_rect.width() > 0.0);
    //     self.view_rect.min.x
    //         + (x_default - Viewport::x_default_from_time(self.time_range.0))
    //             * self.time_scale()
    //             * Self::DEFAULT_TIME_SCALE_DENOM as f32
    // }

    // #[inline]
    // pub fn y_from_default(&self, y: &Pix) -> Pix {
    //     emath::remap(
    //         *y,
    //         Rangef::new(
    //             Viewport::DEFAULT_HALF_TONE_STEP,
    //             Viewport::DEFAULT_HALF_TONE_STEP * STAVE_KEY_LANES.len() as f32,
    //         ),
    //         self.lanes_y_range(),
    //     )
    // }

    pub fn zoom(&mut self, zoom_factor: f32, mouse_x: Pix) {
        // Zoom so that time position under mouse pointer stays put.
        // TODO (cleanup) Consider using emath::remap
        let at = self.time_from_x(mouse_x);
        self.time_range.0 = (at - ((at - self.time_range.0) as f32 / zoom_factor) as Time)
            .max(-Self::ZOOM_TIME_LIMIT)
            - 1;
        self.time_range.1 = (at + ((self.time_range.1 - at) as f32 / zoom_factor) as Time)
            .min(Self::ZOOM_TIME_LIMIT)
            + 1;
        assert!(self.time_range.0 < self.time_range.1)
    }

    pub fn scroll(&mut self, dt: Time) {
        if self.time_range.0 + dt < -Self::ZOOM_TIME_LIMIT
            || self.time_range.1 + dt > Self::ZOOM_TIME_LIMIT
        {
            return;
        }
        self.time_range.0 += dt;
        self.time_range.1 += dt;
    }

    pub fn scroll_by(&mut self, dx: Pix) {
        self.scroll((dx / self.time_scale()) as Time);
    }

    pub fn scroll_to(&mut self, at: Time, view_fraction: f32) {
        self.scroll(
            at - (self.time_range.len() as f32 * view_fraction) as Time - self.time_range.0,
        );
    }
}

// #[derive(Debug)]
pub struct Stave {
    pub history: Arc<RwLock<TrackHistory>>,

    pub viewport: Viewport,

    pub cursor_position: Time,
    pub time_selection: Option<Range<Time>>,
    /// Currently drawn new note.
    pub note_draw: Option<NoteDraw>,
    pub note_selection: NotesSelection,
    /// Change animation parameters.
    pub transition: Option<EditTransition>,

    // Velocity -> note_color lookup map
    note_colors: Vec<Color32>,

    // Experimental
    // Using SyncCOW to support multithreaded mesh generation. Is not needed otherwise.
    // TODO Multithreaded generation does help to offload from single cpu but is rather inefficient still.
    //   Now I wan to experiment with on-demand mesh generation and using lerp on mesh itself for animation
    //   (because I am curious, that is why):
    //   * During transitions use 2 meshes "before" and "after" and generate the output by interpolation for animation.
    //   * Only re-generate mesh when track changes.
    //   * Scale the mesh itself to support zoom/scroll. Maybe this would look like:
    //             out := lerp(before, after); zoom(&mut out)
    //   * When showing and edit action, generate another mesh with the new state.
    //      * Update "before" one with placeholders for insertions.
    //      * After mesh would have placeholders for deletions.
    //   This would require:
    //   * To tessellate track at some default scale.
    //   * Ensure size(before.vertices) == size(after.vertices), deleted state can be represented as completely transparent color.
    //   * When animation finishes, before := after; discard_deleted(&mut before); truncate(&mut after).
    //   * The generated mesh can include notes and CC. I would not want to scale bookmarks or text.
    //   * No need to allocate meshes every time
    meshes: RefCell<Meshes>,
}

/**
Track view model.
Mesh data flow: (a, b) -> interpolate -> unscaled -> vertical-remap -> scaled_y -> zoom-scroll -> out -> render
This is split into stages to shift most frequent updates later so we can re-use the calculations.
*/
#[derive(Default, Clone)]
struct Meshes {
    // TODO (cleanup) This change tracing becomes a bit tedious. Is there a generic implementation?
    // Track version id, used to detect changes in events (edit changes).
    version_id: VersionId,
    // Track selection changes.
    selection_version: u64,
    time_selection: Option<Range<Time>>,

    // TODO (cleanup) This map can be cached in the tack itself, but need to ensure
    //      it is updated after each track change.
    events: HashMap<EventId, TrackEvent>,

    // Beginning and end of ongoing animation.
    transition: Option<(Mesh, Mesh)>,

    // Evens painted at current vertical and horizontal scale, without xy shifts (at 0,0).
    scaled: Mesh,

    // Horizontal/vertical scaling.
    height: Pix,
    time_scale: f64,
    // Scroll/translation.
    time_start: Time,
    xy: Pos2,

    // Annotates resulting triangles with track event ids.
    // Used to detect hovers, and to show out-of-view selection hints.
    out_events: Vec<EventId>,
    out: Arc<Mesh>,
}

// Holds time (x) and vertical (y, lanes) scaling for stave.
struct TYScale {
    // Pix/uSec
    time_scale: f64,
    // Y step per lane
    y_step: Pix,
}

impl TYScale {
    #[inline]
    pub fn x(&self, at: &Time) -> Pix {
        (*at as f64 * self.time_scale) as Pix
    }

    #[inline]
    pub fn y(&self, pitch: &Pitch) -> f32 {
        (STAVE_KEY_LANES.len() + PIANO_LOWEST_KEY - pitch - CONTROL_LANES_COUNT) as Pix
            * self.y_step
            - self.y_step / 2.0
    }
}

const COLOR_SELECTED: Rgba = Rgba::from_rgb(0.7, 0.1, 0.3);
const COLOR_HOVERED: Rgba = Rgba::from_rgb(0.2, 0.5, 0.55);

// Egui optimizes away transparent shapes. This placeholder color is used as starting or end point
// in insertions/deletions to ensure the shape is always there.
const COLOR_NOTHING: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 1);
// const COLOR_NOTHING: Color32 = Color32::GREEN; // DEBUG // Making it stand out for diagnostics.

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
    pub fn new(history: Arc<RwLock<TrackHistory>>) -> Stave {
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
            viewport: Viewport {
                time_range: (0, chrono::Duration::minutes(5).num_microseconds().unwrap()),
                view_rect: Rect::NOTHING,
            },
            cursor_position: 0,
            time_selection: None,
            note_draw: None,
            note_selection: NotesSelection::default(),
            transition: None,
            note_colors,
            meshes: RefCell::new(Meshes::default()),
        }
    }

    pub fn save_to(&mut self, file_path: &PathBuf) {
        self.history
            .read()
            .expect("Read stave.history.")
            .with_track(|track| export_smf(&track.events, file_path));
    }

    pub fn zoom_to_fit(&mut self, time_margin: Time) {
        self.viewport.time_range = (
            -time_margin,
            self.history
                .read()
                .expect("Read stave.history.")
                .with_track(|tr| tr.max_time())
                + time_margin,
        );
    }

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
                    self.viewport.view_rect = bounds;

                    ruler_rect.set_height(ruler_height);
                    // TODO (cleanup) Use painter_at instead.
                    self.draw_time_ruler(&ui.painter(), ruler_rect);
                }

                let (key_ys, half_tone_step) = key_line_ys(&bounds.y_range(), STAVE_KEY_LANES);
                let mut pitch_hovered = None;
                let mut time_hovered = None;
                let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                if let Some(pointer_pos) = pointer_pos {
                    pitch_hovered = Some(closest_pitch(&key_ys, pointer_pos));
                    time_hovered = Some(self.viewport.time_from_x(pointer_pos.x));
                }

                let painter = ui.painter_at(bounds);
                Self::draw_grid(&painter, bounds, &key_ys, &pitch_hovered);
                let selection_color = Color32::from_rgba_unmultiplied(64, 80, 100, 60);
                if let Some(s) = &self.time_selection {
                    self.draw_time_selection(&painter, &s, &selection_color);
                }
                self.draw_time_selection(
                    &painter,
                    &(self.viewport.time_range.0, 0),
                    &Color32::from_black_alpha(15),
                );
                let mut note_hovered = None;
                let should_be_visible;
                {
                    let history = self.history.read().expect("Read stave.history.");
                    let track = history.track.read();
                    should_be_visible = self.paint_events(
                        &key_ys,
                        &half_tone_step,
                        &pointer_pos,
                        &mut note_hovered,
                        &painter,
                        history.version(),
                        &track,
                    );
                }
                painter.add(self.cursor_shape(
                    &painter.clip_rect().y_range(),
                    self.viewport.x_from_time(self.cursor_position),
                    Rgba::from_rgba_unmultiplied(0.0, 0.5, 0.0, 0.7).into(),
                ));

                if let Some(new_note) = &self.note_draw {
                    painter.add(self.default_draw_note(
                        64,
                        (new_note.time.0, new_note.time.1),
                        *key_ys.get(&new_note.pitch).unwrap(),
                        half_tone_step,
                        true,
                    ));
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

    const TIME_RULER_TICK_DURATIONS_SECONDS: [f32; 16] = [
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

    fn draw_time_ruler(&mut self, painter: &Painter, ruler_rect: Rect) {
        let time_width = ruler_rect.width() / self.viewport.time_scale();
        let tick_duration = Self::TIME_RULER_TICK_DURATIONS_SECONDS
            .iter()
            .find_map(|td| {
                let x = td * 1_000_000.0; // From seconds
                let n_ticks = time_width / x;
                if 2.0 < n_ticks && n_ticks < 20.0 {
                    Some(x)
                } else {
                    None
                }
            })
            .unwrap_or(time_width / 5.0)
            .round() as Time;
        assert!(tick_duration > 0);
        let start_tick = self.viewport.time_from_x(ruler_rect.min.x) / tick_duration;
        let end_tick = self.viewport.time_from_x(ruler_rect.max.x) / tick_duration;
        let mut last_x = self.viewport.x_from_time(-1);
        for tick in start_tick..end_tick + 1 {
            let at = tick * tick_duration;
            // Avoids labels overlapping.
            if last_x < self.viewport.x_from_time(at) {
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
        result.push_str(&format!(
            "{}'{}",
            minutes,
            seconds as f32 + millis as f32 / 1000.0
        ));
        result
    }

    fn draw_time_tick(&mut self, painter: &Painter, ruler_rect: Rect, at: Time) -> Rect {
        let x = self.viewport.x_from_time(at);
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

    fn shift_mesh(mesh: &mut Mesh, by: &Vec2) {
        for v in &mut mesh.vertices {
            v.pos += *by;
        }
    }

    fn paint_events(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        pointer_pos: &Option<Pos2>,
        note_hovered: &mut Option<EventId>,
        painter: &Painter,
        version_id: VersionId,
        track: &Track,
    ) -> Option<range::Range<Time>> {
        let x_range = painter.clip_rect().x_range();
        // Makes elements affected by animation visible,
        // to help avoiding inadvertent changes.
        let mut should_be_visible = None;

        let font_tex_size = [0, 0]; // unused
        let prepared_discs = vec![]; // unused

        // TODO Use tesselator scale from settings.
        let tessel_options = TessellationOptions::default();
        let mut tessellator = Tessellator::new(1.0, tessel_options, font_tex_size, prepared_discs);

        let mut meshes = self.meshes.borrow_mut();
        let has_version_changed = meshes.version_id != version_id;
        let has_selection_changed = meshes.selection_version != self.note_selection.version
            || meshes.time_selection != self.time_selection;

        let height = self.viewport.view_rect.height();
        let ty = TYScale {
            y_step: height / STAVE_KEY_LANES.len() as f32,
            time_scale: self.viewport.view_rect.width() as f64
                / self.viewport.time_range.len() as f64,
        };
        let has_scale_changed = height != meshes.height || ty.time_scale != meshes.time_scale;
        let mut animation_bounds = Rect::NOTHING;

        if has_version_changed || has_scale_changed || has_selection_changed {
            meshes.events = track.index_events();
            // (!) Assuming vertice indices will not change in subsequent transformations.
            meshes.out_events.clear();
            meshes.scaled.clear();

            if let Some(trans) = &self.transition {
                let mut before = Mesh::default();
                let mut after = Mesh::default();
                for (_ev_id, action) in &trans.changeset.changes {
                    if let Some((shape_a, shape_b)) = self.note_animation(action, &ty) {
                        tessellator.tessellate_shape(shape_a, &mut before);
                        tessellator.tessellate_shape(shape_b, &mut after);
                    } else if let Some((shape_a, shape_b)) = self.cc_animation(action, &ty) {
                        tessellator.tessellate_shape(shape_a, &mut before);
                        tessellator.tessellate_shape(shape_b, &mut after);
                    } else {
                        // E.g. a bookmark.
                    }
                }
                // Current animation procedure assumes that only vertices change.
                // Hence, "before" to "after" mapping should be 1 to 1.
                assert_eq!(before.indices.len(), after.indices.len());
                assert_eq!(before.vertices.len(), after.vertices.len());

                if !before.is_empty() {
                    // Only triggering this once per edit, it seems to be sufficient.
                    bounding_rect(&before, &mut animation_bounds);
                    bounding_rect(&after, &mut animation_bounds);
                }
                meshes.transition = Some((before, after));
            }

            let mut last_damper_value: (Time, Level) = (0, DEFAULT_CC_LEVEL);
            for event in &track.events {
                if meshes.transition.is_some() {
                    if let Some(trans) = &self.transition {
                        if trans.changeset.changes.contains_key(&event.id) {
                            continue;
                        }
                    }
                }
                match &event.event {
                    TrackEventType::Note(note) => {
                        let shape = self.note_event_shape(&event, &note, &ty);
                        tessellator.tessellate_shape(shape, &mut meshes.scaled);
                    }
                    TrackEventType::Controller(cc) => {
                        if let Some(shape) =
                            self.default_track_cc_shape(&mut last_damper_value, &event, &cc, &ty)
                        {
                            tessellator.tessellate_shape(shape, &mut meshes.scaled);
                        }
                    }
                    TrackEventType::Bookmark => {
                        let shape = self.cursor_shape(
                            &Rangef::new(0.0, self.viewport.view_rect.y_range().span()),
                            ty.x(&event.at),
                            Rgba::from_rgba_premultiplied(0.0, 0.4, 0.0, 0.3).into(),
                        );
                        tessellator.tessellate_shape(shape, &mut meshes.scaled);
                    }
                }
                while meshes.out_events.len() < meshes.scaled.indices.len() / 3 {
                    meshes.out_events.push(event.id);
                }
            }
            assert!(meshes.out_events.len() <= meshes.scaled.indices.len() / 3);
            meshes.version_id = version_id;
            meshes.selection_version = self.note_selection.version;
            meshes.time_selection = self.time_selection;
            meshes.height = height;
            meshes.time_scale = ty.time_scale;
        }

        let mut animated = Mesh::default();
        if let Some(EditTransition { coeff, .. }) = self.transition {
            debug_assert!(0.0 <= coeff && coeff <= 1.0);
            debug_assert!(meshes.transition.is_some());
            if let Some((mesh_a, mesh_b)) = &meshes.transition {
                animated.clone_from(mesh_a);
                animated.vertices.clear();
                for (va, vb) in mesh_a.vertices.iter().zip(mesh_b.vertices.iter()) {
                    animated.vertices.push(Vertex {
                        pos: Pos2 {
                            x: emath::lerp(va.pos.x..=vb.pos.x, coeff),
                            y: emath::lerp(va.pos.y..=vb.pos.y, coeff),
                        },
                        uv: va.uv,
                        color: emath::lerp(Rgba::from(va.color)..=Rgba::from(vb.color), coeff)
                            .into(),
                    });
                }
                debug_assert!(animated.is_valid());
            }
        } else if meshes.transition.is_some() {
            // Animation end.
            meshes.transition = None;
            meshes.version_id = -1; // Force repaint without animation parts next time.
        }

        let is_animating = !animated.is_empty();
        // Horizontal zoom and scrolling.
        let has_position_changed = meshes.xy != self.viewport.view_rect.min
            || meshes.time_start != self.viewport.time_range.0;
        // Diff between 0,0 origin of default mesh rendering and viewport position.
        let shift = self.viewport.view_rect.min.to_vec2()
            // Time scrolling
            + Vec2 {
                x: -ty.x(&self.viewport.time_range.0),
                y: 0f32,
            };
        if is_animating && animation_bounds != Rect::NOTHING {
            should_be_visible = Some((
                self.viewport.time_from_x(animation_bounds.min.x + shift.x),
                self.viewport.time_from_x(animation_bounds.max.x + shift.x),
            ));
        }
        if has_version_changed
            || has_scale_changed
            || has_selection_changed
            || has_position_changed
            || is_animating
        {
            let mut mesh = Mesh::default();
            mesh.clone_from(&meshes.scaled);
            mesh.append(animated);
            Self::shift_mesh(&mut mesh, &shift);
            meshes.xy = self.viewport.view_rect.min;
            meshes.time_start = self.viewport.time_range.0;
            // debug_assert_eq!(meshes.viewport.view_rect, self.viewport.view_rect);
            meshes.out = Arc::new(mesh);
        }
        painter.add(Shape::mesh(meshes.out.to_owned()));

        // Contains shapes that may change every frame, not caching these.
        let mut transients_mesh = Mesh::default();
        // TODO (optimization) Linear lookup (see also selection hints). This and other parts of
        //   this pipeline can be optimized with a spatial tree, but it is fast enough for now.
        if let Some(&pointer_pos) = pointer_pos.as_ref() {
            // Hover
            for triangle in 0..meshes.out_events.len() {
                let event_id = meshes.out_events[triangle];
                if point_inside_mesh_triangle(&meshes.out, triangle, pointer_pos) {
                    *note_hovered = Some(event_id);
                    let ev = meshes
                        .events
                        .get(&event_id)
                        .expect("hovered event is on the track");
                    if let Some(hover_shape) = Stave::hover_shape(&ev, &ty) {
                        tessellator.tessellate_shape(hover_shape, &mut transients_mesh);
                    }
                    break;
                }
            }
        }
        /* Paint some sign at the visible border of the stave to hint that
        there are also selected events that are not currently visible,
        This should help avoiding inadvertent edits. */
        let mut selection_hints_left: HashSet<Pitch> = HashSet::new();
        let mut selection_hints_right: HashSet<Pitch> = HashSet::new();
        for event in &track.events {
            if let TrackEventType::Note(note) = &event.event {
                if self.note_selection.contains(&event.id) {
                    // // Visibility can also be determined by comparing triangles coordinate's.
                    if x_range.max < ty.x(&event.at) + shift.x {
                        selection_hints_right.insert(note.pitch);
                    } else if ty.x(&(event.at + note.duration)) + shift.x < x_range.min {
                        selection_hints_left.insert(note.pitch);
                    }
                }
            }
        }
        Self::shift_mesh(&mut transients_mesh, &shift);
        painter.add(Shape::mesh(transients_mesh));

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
        if let Some(transition) = &mut self.transition {
            transition.update(&ui.ctx())
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

        let had_transition = self.transition.is_some();
        self.transition = self.transition.take().filter(|tr| tr.value().is_some());
        if had_transition && self.transition.is_none() {
            ui.ctx().clear_animations();
            ui.ctx().request_repaint();
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
                Modifiers::NONE,
                egui::Key::CloseBracket,
            ))
        }) {
            if let Some(time_selection) = &self.time_selection.clone() {
                self.adjust_tempo(&response, &time_selection, 1.01);
            }
        }
        if response.ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::NONE,
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
                let id_seq = &self
                    .history
                    .read()
                    .expect("Read stave.history.")
                    .id_seq
                    .clone();
                self.do_edit_command(&response.ctx, response.id, |_stave, track| {
                    tape_delete(id_seq, track, &(time_selection.0, time_selection.1))
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
            let edit_state = if self
                .history
                .write()
                .expect("Write stave.history.")
                .undo(&mut changes)
            {
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
            let edit_state = if self
                .history
                .write()
                .expect("Write stave.history.")
                .redo(&mut changes)
            {
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
            let id_seq = &self
                .history
                .read()
                .expect("Read stave.history.")
                .id_seq
                .clone();
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
                .read()
                .expect("Read stave history.")
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
                .read()
                .expect("Read stave history.")
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
                .read()
                .expect("Read stave.history.")
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
                .read()
                .expect("Read stave.history.")
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
                let at = self.viewport.time_from_x(hover_pos.x);
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
            .write()
            .expect("Write stave.history.")
            .update_track(|track| action(&self, track));
        self.transition = Self::animate_edit(
            context,
            transition_id,
            diff.clone().map(|diff| (diff.0.0, diff.1)),
        );
        diff
    }

    fn max_time(&self) -> Time {
        self.history
            .read()
            .expect("Read stave.history.")
            .with_track(|track| track.max_time())
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
                    let id_seq = &self
                        .history
                        .read()
                        .expect("Read stave.history.")
                        .id_seq
                        .clone();
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

    fn cursor_shape(&self, y_range: &Rangef, x: Pix, color: Color32) -> Shape {
        Shape::vline(x, *y_range, Stroke { width: 2.0, color })
    }

    fn note_animation(&self, action: &EventAction, ty: &TYScale) -> Option<(Shape, Shape)> {
        debug_assert!(COLOR_NOTHING != Color32::TRANSPARENT);
        match (action.before(), action.after()) {
            (
                Some(TrackEvent {
                    id: id_a,
                    at: at_a,
                    event: TrackEventType::Note(a),
                }),
                Some(TrackEvent {
                    id: id_b,
                    at: at_b,
                    event: TrackEventType::Note(b),
                }),
            ) => Some((
                self.track_note_shape(id_a, at_a, a, None, ty),
                self.track_note_shape(id_b, at_b, b, None, ty),
            )),
            // New note
            (
                None,
                Some(TrackEvent {
                    id: id_b,
                    at: at_b,
                    event: TrackEventType::Note(b),
                }),
            ) => Some((
                self.track_note_shape(id_b, at_b, b, Some(COLOR_NOTHING), ty),
                self.track_note_shape(id_b, at_b, b, None, ty),
            )),
            // Deleted note
            (
                Some(TrackEvent {
                    id: id_a,
                    at: at_a,
                    event: TrackEventType::Note(a),
                }),
                None,
            ) => Some((
                self.track_note_shape(id_a, at_a, a, None, ty),
                self.track_note_shape(id_a, at_a, a, Some(COLOR_NOTHING), ty),
            )),
            _ => None,
        }
    }

    fn note_color(&self, velocity: &Level, selected: bool) -> Color32 {
        if selected {
            COLOR_SELECTED.into()
        } else {
            self.note_colors[*velocity as usize]
        }
    }

    // TODO (cleanup) Remove now redundant painting procedures
    fn note_event_shape(&self, event: &TrackEvent, note: &Note, ty: &TYScale) -> Shape {
        self.track_note_shape(&event.id, &event.at, note, None, ty)
    }

    fn track_note_shape(
        &self,
        event_id: &EventId,
        at: &Time,
        note: &Note,
        color: Option<Color32>,
        ty: &TYScale,
    ) -> Shape {
        Self::note_shape(
            (*at, at + note.duration),
            &note.pitch,
            color.unwrap_or(
                self.note_color(&note.velocity, self.note_selection.contains(&event_id)),
            ),
            ty,
        )
    }

    fn note_shape(time_range: (Time, Time), pitch: &Pitch, color: Color32, ty: &TYScale) -> Shape {
        Shape::Rect(RectShape::filled(
            Self::event_rect(time_range, &pitch, ty),
            CornerRadius::ZERO,
            color,
        ))
    }

    fn event_rect(time_range: (Time, Time), pitch: &Pitch, ty: &TYScale) -> Rect {
        let y = ty.y(&pitch);
        Rect {
            min: Pos2 {
                x: ty.x(&time_range.0),
                y: y - ty.y_step * 0.45,
            },
            max: Pos2 {
                x: ty.x(&time_range.1),
                y: y + ty.y_step * 0.45,
            },
        }
    }

    fn hover_shape(event: &TrackEvent, ty: &TYScale) -> Option<Shape> {
        match &event.event {
            TrackEventType::Note(note) => Some(Shape::Rect(RectShape::stroke(
                Self::event_rect((event.at, event.at + note.duration), &note.pitch, ty),
                CornerRadius::ZERO,
                Stroke::new(2.0, COLOR_HOVERED),
                StrokeKind::Middle,
            ))),
            _ => None,
        }
    }

    // To draw note immediately (without intermediate meshes).
    fn paint_note(&self, time_range: (Time, Time), y: Pix, height: Pix, color: Color32) -> Shape {
        let paint_rect = Rect {
            min: Pos2 {
                x: self.viewport.x_from_time(time_range.0),
                y: y - height * 0.45,
            },
            max: Pos2 {
                x: self.viewport.x_from_time(time_range.1),
                y: y + height * 0.45,
            },
        };
        Shape::Rect(RectShape::filled(paint_rect, CornerRadius::ZERO, color))
    }

    fn default_draw_note(
        &self,
        velocity: Level,
        x_range: (Time, Time),
        y: Pix,
        height: Pix,
        selected: bool,
    ) -> Shape {
        self.paint_note(x_range, y, height, self.note_color(&velocity, selected))
    }

    fn point_accent_shape(time: &Time, pitch: &Pitch, color: Color32, ty: &TYScale) -> Shape {
        Shape::circle_filled(
            Pos2 {
                x: ty.x(time),
                y: ty.y(pitch),
            },
            ty.y_step / 2.2,
            color,
        )
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
                x: self.viewport.x_from_time(time),
                y,
            },
            height / 2.2,
            color,
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

    fn cc_animation(&self, action: &EventAction, ty: &TYScale) -> Option<(Shape, Shape)> {
        match (action.before(), action.after()) {
            (
                Some(TrackEvent {
                    id: _id_a,
                    at: at_a,
                    event: TrackEventType::Controller(a),
                }),
                Some(TrackEvent {
                    id: _id_b,
                    at: at_b,
                    event: TrackEventType::Controller(b),
                }),
            ) => Some((
                Self::point_accent_shape(
                    at_a,
                    &PIANO_DAMPER_LANE,
                    self.note_color(&a.value, false),
                    ty,
                ),
                Self::point_accent_shape(
                    at_b,
                    &PIANO_DAMPER_LANE,
                    self.note_color(&b.value, false),
                    ty,
                ),
            )),
            // New CC value
            (
                None,
                Some(TrackEvent {
                    id: _id_b,
                    at: at_b,
                    event: TrackEventType::Controller(b),
                }),
            ) => Some((
                Self::point_accent_shape(
                    at_b,
                    &PIANO_DAMPER_LANE,
                    self.note_color(&b.value, false),
                    ty,
                ),
                Self::point_accent_shape(
                    at_b,
                    &PIANO_DAMPER_LANE,
                    self.note_color(&b.value, false),
                    ty,
                ),
            )),
            // Deleted value
            (
                Some(TrackEvent {
                    id: _id_a,
                    at: at_a,
                    event: TrackEventType::Controller(a),
                }),
                None,
            ) => Some((
                Self::point_accent_shape(
                    at_a,
                    &PIANO_DAMPER_LANE,
                    self.note_color(&a.value, false),
                    ty,
                ),
                Self::point_accent_shape(
                    at_a,
                    &PIANO_DAMPER_LANE,
                    self.note_color(&a.value, false),
                    ty,
                ),
            )),
            _ => None,
        }
    }

    fn default_track_cc_shape(
        &self,
        last_damper_value: &mut (Time, Level),
        event: &TrackEvent,
        cc: &ControllerSetValue,
        ty: &TYScale,
    ) -> Option<Shape> {
        if cc.controller_id == MIDI_CC_SUSTAIN_ID {
            let shape = Self::note_shape(
                (last_damper_value.0, event.at),
                &PIANO_DAMPER_LANE,
                self.note_color(&last_damper_value.1, false),
                ty,
            );
            *last_damper_value = (event.at, cc.value);
            Some(shape)
        } else {
            None
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
                x: self.viewport.x_from_time(selection.0),
                y: clip.min.y,
            },
            max: Pos2 {
                x: self.viewport.x_from_time(selection.1),
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
        let x_range = self.viewport.view_rect.x_range();
        let x = self.viewport.x_from_time(at);
        if !x_range.contains(x) {
            if x_range.max < x {
                self.viewport.scroll_to(at, 0.7);
            } else {
                self.viewport.scroll_to(at, 0.3);
            }
        }
    }

    fn is_visible(&self, at: Time) -> bool {
        self.viewport
            .view_rect
            .x_range()
            .contains(self.viewport.x_from_time(at))
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
        .min_by_key(|(_, y)| OrderedFloat((**y - pointer_pos.y).abs()))
        .unwrap()
        .0
}

// Barycentric sign test
#[inline]
fn cross_prod(u: Vec2, v: Vec2) -> f32 {
    u.x * v.y - u.y * v.x
}

#[inline]
fn point_in_triangle(p: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let c1 = cross_prod(b - a, p - a);
    let c2 = cross_prod(c - b, p - b);
    let c3 = cross_prod(a - c, p - c);

    (c1 >= 0.0 && c2 >= 0.0 && c3 >= 0.0) || (c1 <= 0.0 && c2 <= 0.0 && c3 <= 0.0)
}

// #[inline]
// fn nth_triangle_indices(mesh: &Mesh, triangle_idx: usize) -> (u32, u32, u32) {
//     let idx = triangle_idx * 3;
//     (
//         mesh.indices[idx],
//         mesh.indices[idx + 1],
//         mesh.indices[idx + 2],
//     )
// }

#[inline]
fn point_inside_mesh_triangle(mesh: &Mesh, triangle_idx: usize, p: Pos2) -> bool {
    let idx = triangle_idx * 3;
    let a = mesh.vertices[mesh.indices[idx] as usize].pos;
    let b = mesh.vertices[mesh.indices[idx + 1] as usize].pos;
    let c = mesh.vertices[mesh.indices[idx + 2] as usize].pos;
    point_in_triangle(p, a, b, c)
}

fn bounding_rect(mesh: &Mesh, bounds: &mut Rect) {
    assert!(!mesh.vertices.is_empty());
    for v in &mesh.vertices[0..] {
        let p = v.pos;
        bounds.min.x = bounds.min.x.min(p.x);
        bounds.min.y = bounds.min.y.min(p.y);
        bounds.max.x = bounds.max.x.max(p.x);
        bounds.max.y = bounds.max.y.max(p.y);
    }
}

#[cfg(test)]
mod tests {
    use super::Viewport;
    use eframe::egui::{Pos2, Rect};

    #[test]
    fn check_default_lanes_y_scaling() {
        let viewport = Viewport {
            time_range: (0, 2000),
            view_rect: Rect {
                min: Pos2 { x: 0.0, y: 0.0 },
                max: Pos2 { x: 200.0, y: 108.0 },
            },
        };
        // dbg!(Viewport::pitch_y_default(&PIANO_KEY_LINES.0));
        // dbg!(Viewport::pitch_y_default(&PIANO_KEY_LINES.1));
        // let (pitches, step) = super::key_line_ys(&viewport.view_rect.y_range(), STAVE_KEY_LANES);
        // // dbg!(step, &pitches);
        // assert_eq!(
        //     Viewport::pitch_y_default(&PIANO_LOWEST_KEY),
        //     Viewport::DEFAULT_HALF_TONE_STEP * PIANO_KEY_COUNT as f32
        // );
        // assert_eq!(
        //     Viewport::pitch_y_default(&(PIANO_LOWEST_KEY + PIANO_KEY_COUNT)),
        //     0.0
        // );
        // for p in STAVE_KEY_LANES.range() {
        //     let pitch_y = pitches.get(&p);
        //     let translated_y = viewport.y_from_default(&Viewport::pitch_y_default(&p));
        //     dbg!(
        //         p,
        //         &pitch_y.unwrap(),
        //         translated_y,
        //         pitch_y.unwrap() - translated_y
        //     );
        //     assert!((pitch_y.unwrap() - &translated_y).abs() < 0.05f32);
        // }
    }

    #[test]
    fn check_time_x_scaling() {
        let viewport = Viewport {
            time_range: (0, 2_345_678),
            view_rect: Rect {
                min: Pos2 { x: 0.0, y: 0.0 },
                max: Pos2 {
                    x: 1000.0,
                    y: 108.0,
                },
            },
        };
        for t in [-13, 0, 1234, 23, 100006] {
            assert!((viewport.time_from_x(viewport.x_from_time(t)) - t).abs() <= 1);
        }
        for x in [-3.3, 0.0, 0.01, 0.044, 134.0, 1.8765444e7] {
            assert!((viewport.x_from_time(viewport.time_from_x(x)) - x).abs() < 1e-3);
        }

        // TODO Check key_ys and pre-render events ys match.
        // dbg!(
        //     (Viewport::ZOOM_TIME_LIMIT / (1i64 << Pix::MANTISSA_DIGITS)),
        //     Viewport::DEFAULT_TIME_SCALE_DENOM
        // );
        // assert!(
        //     (Viewport::ZOOM_TIME_LIMIT / (1i64 << Pix::MANTISSA_DIGITS))
        //         < Viewport::DEFAULT_TIME_SCALE_DENOM
        // );

        // for t in [
        //     -123454321,
        //     -33001,
        //     0,
        //     33,
        //     1564,
        //     7000,
        //     8321,
        //     123_000_098,
        //     Viewport::ZOOM_TIME_LIMIT,
        // ] {
        //     let tt = viewport.x_from_time(Viewport::x_default_from_time(t));
        //     assert!(
        //         (viewport.x_from_default(Viewport::x_default_from_time(t))
        //             - viewport.x_from_time(t))
        //         .abs()
        //             < 0.01
        //     );
        // }
    }
}

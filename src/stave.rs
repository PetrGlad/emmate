use crate::changeset::{Changeset, EventAction, EventActionsList};
use crate::common::{Time, VersionId};
use crate::range::{Range, RangeLike, RangeSpan};
use crate::track::{
    export_smf, ControllerSetValue, EventId, Level, Note, Pitch, Track, TrackEvent, TrackEventType,
    DEFAULT_CC_LEVEL, MAX_LEVEL, MIDI_CC_SUSTAIN_ID,
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
    self, Align2, Color32, Context, CornerRadius, FontId, Frame, Margin, Mesh, Modifiers, Painter,
    PointerButton, Pos2, Rangef, Rect, Response, Sense, Shape, Stroke, Ui, Vec2,
};
use eframe::emath;
use eframe::emath::TSTransform;
use eframe::epaint::{
    ClippedShape, RectShape, StrokeKind, TessellationOptions, Tessellator, Vertex,
};
use egui::Rgba;
use ordered_float::OrderedFloat;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use sync_cow::SyncCow;
use tracing::Event;

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
}

impl NotesSelection {
    fn toggle(&mut self, id: &EventId) {
        if self.selected.contains(&id) {
            self.selected.remove(&id);
        } else {
            self.selected.insert(*id);
        }
    }

    fn contains(&self, ev_id: &EventId) -> bool {
        self.selected.contains(ev_id)
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

// Selects viewable track range.
type TimeRange = Range<Time>;

#[derive(Clone, PartialEq, Eq)]
pub struct Viewport {
    /// Starting and ending moment of track's visible time range.
    pub time_range: TimeRange,
    /// The widget's displayed rectangle coordinates.
    pub view_rect: Rect,
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

    // Some reasonable scale to fit ZOOM_TIME_LIMIT into Pix for pre-backed meshes.
    const DEFAULT_TIME_SCALE: Pix =
        1.0 / (Self::ZOOM_TIME_LIMIT / (1i64 << Pix::MANTISSA_DIGITS)) as Pix;
    const DEFAULT_TIME_SCALE_DBG_INV: Time = 1000;
    // 1.0 / (Self::ZOOM_TIME_LIMIT / (1i64 << Pix::MANTISSA_DIGITS)) as Pix;

    const DEFAULT_HALF_TONE_STEP: Pix = 10.0;

    /// Pixel/uSec, can be cached.
    #[inline]
    pub fn time_scale(&self) -> f32 {
        debug_assert!(self.view_rect.width() > 0.0);
        self.view_rect.width() / self.time_range.len() as f32
    }

    #[inline]
    pub fn x_from_time(&self, at: Time) -> Pix {
        debug_assert!(self.view_rect.width() > 0.0);
        self.view_rect.min.x + (at as f32 - self.time_range.0 as f32) * self.time_scale()
    }

    #[inline]
    pub fn x_from_default(&self, x_default: Pix) -> Pix {
        debug_assert!(self.view_rect.width() > 0.0);
        self.view_rect.min.x
            + (x_default - (self.time_range.0 / Self::DEFAULT_TIME_SCALE_DBG_INV) as f32)
                * self.time_scale() * Self::DEFAULT_TIME_SCALE_DBG_INV as f32
    }

    pub fn time_from_x(&self, x: Pix) -> Time {
        debug_assert!(self.view_rect.width() > 0.0);
        self.time_range.0 + ((x - self.view_rect.min.x) / self.time_scale()) as Time
    }

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
    meshes: SyncCow<Meshes>,
}

// TODO This should replace EditTransition
struct AnimationTransition {
    a: Mesh,
    b: Mesh,
}

impl AnimationTransition {
    fn from_changeset(changeset: &Changeset) {}

    // Interpolation coefficient [0.0..1.0] between previous and current.
    fn current(animation: f32) {
        // TODO implement interpolation betweeen a and b
    }
}

/**
Track view model.
Mesh data flow: (a, b) -> interpolate -> unscaled -> vertical-remap -> scaled_y -> zoom-scroll -> out -> render
This is split into stages to shift most frequent updates later so we can re-use the calculations.
*/
#[derive(Default, Clone)]
struct Meshes {
    // Tracks changes in events (edit changes).
    version_id: VersionId,

    // Beginning and end of ongoing animation.
    // TODO Use AnimationTransition
    transition: Option<(Mesh, Mesh)>,

    // Track drawn at some default scale that does not depend on current viewport
    // (drawn with a pre-defined view port). This helps to skip notes tesselation when track
    // events have not changed.
    default: Mesh,
    // Optimization: this field is not strictly necessary, but vertical scaling
    // is performed less often, so keeping this partial apply.
    scaled_y: Mesh,
    // Lets tracking changes in view (zoom, scroll).
    viewport: Viewport,
    // Annotates out vertices with tract event ids.
    // Used to detect hovers, and to show out-of-view selection hints.
    out_events: Vec<EventId>,
    out: Arc<Mesh>,
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
            meshes: SyncCow::new(Meshes::default()),
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
                    &Stave::NOTHING_ZONE,
                    &Color32::from_black_alpha(15),
                );
                let mut note_hovered = None;
                let should_be_visible;
                {
                    let history = self.history.read().expect("Read stave.history.");
                    let track = history.track.read();
                    should_be_visible = self.draw_events(
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
        result.push_str(&format!("{}'{}", minutes, seconds));
        if millis > 0 {
            result.push_str(&format!(".{}", millis));
        }
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

    fn draw_events(
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
        let mut selection_hints_left: HashSet<Pitch> = HashSet::new();
        let mut selection_hints_right: HashSet<Pitch> = HashSet::new();
        let mut should_be_visible = None;

        // -----------------------------------------------------------------------------------
        // Experimental

        let font_tex_size = [0, 0]; // unused
        let prepared_discs = vec![]; // unused

        // TODO Use tesselator scale from settings.
        let mut tessel_options = TessellationOptions::default();
        tessel_options.feathering = false;
        let mut tessellator = Tessellator::new(1.0, tessel_options, font_tex_size, prepared_discs);

        // FIXME (edit transitions) Only using `b` side for now (no transitions/interpolation).
        self.meshes.edit(|meshes: &mut Meshes| {
            let has_version_changed = meshes.version_id != version_id;
            if has_version_changed {
                // (!) Assuming vertice indices will not change in subsequent transformations.
                meshes.out_events.clear();
                meshes.default.clear();
                let mut last_damper_value: (Time, Level) = (0, DEFAULT_CC_LEVEL);
                for event in &track.events {
                    if let Some(trans) = &self.transition {
                        if trans.changeset.changes.contains_key(&event.id) {
                            continue;
                        }
                    }
                    match &event.event {
                        TrackEventType::Note(note) => {
                            let shape = self.track_note_shape_default(&event, &note);
                            tessellator.tessellate_shape(shape, &mut meshes.default);
                        }
                        TrackEventType::Controller(cc) => {
                            // TODO Restore CC display, use the returned shape (painter should not be used anymore)
                            if let Some(shape) = self.draw_track_cc(
                                &key_ys,
                                half_tone_step,
                                &mut last_damper_value,
                                &event,
                                &cc,
                            ) {
                                tessellator.tessellate_shape(shape, &mut meshes.default);
                            }
                        }
                        // TODO Draw cursors separately? I would rather not to scale them.
                        TrackEventType::Bookmark => {
                            let shape = self.cursor_shape(
                                &Rangef::new(
                                    0.0,
                                    // TODO (refactoring) Cleanuo lanes y calculations
                                    (PIANO_KEY_COUNT + PIANO_LOWEST_KEY) as Pix
                                        * Viewport::DEFAULT_HALF_TONE_STEP,
                                ),
                                self.viewport.x_from_time(event.at),
                                Rgba::from_rgba_unmultiplied(0.0, 0.4, 0.0, 0.3).into(),
                            );
                            tessellator.tessellate_shape(shape, &mut meshes.default);
                        }
                    }
                    while meshes.out_events.len() < meshes.default.indices.len() / 3 {
                        meshes.out_events.push(event.id);
                    }
                }
                assert!(meshes.out_events.len() <= meshes.default.indices.len() / 3);

                if let Some(trans) = &self.transition {
                    let mut before = Mesh::default();
                    let mut after = Mesh::default();
                    for (_ev_id, action) in &trans.changeset.changes {
                        if let Some((shape_a, shape_b)) = self.note_animation(action) {
                            tessellator.tessellate_shape(shape_a, &mut before);
                            tessellator.tessellate_shape(shape_b, &mut after);
                        } else if let Some((shape_a, shape_b)) = self.cc_animation(action) {
                            tessellator.tessellate_shape(shape_a, &mut before);
                            tessellator.tessellate_shape(shape_b, &mut after);
                        } else {
                            // TODO (implementation) Handle bookmarks (can be either animated somehow or just ignored).
                            log::trace!("No animation params (a bookmark?).");
                        }
                    }
                    // Current animation procedure assumes that only vertices change,
                    // so before to after mapping should be 1 to 1.
                    assert_eq!(before.indices.len(), before.indices.len());
                    assert_eq!(before.vertices.len(), before.vertices.len());
                    meshes.transition = Some((before, after));
                } else {
                    meshes.transition = None;
                }

                meshes.version_id = version_id;
            }

            let mut animated = Mesh::default();
            if let Some(EditTransition { coeff, .. }) = self.transition {
                debug_assert!(0.0 <= coeff && coeff <= 1.0);
                if let Some((mesh_a, mesh_b)) = &meshes.transition {
                    animated.clone_from(mesh_a);
                    animated.vertices.clear();
                    // FIXME Interpolate here into `animated`.
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
                }
            }
            let is_animating = !animated.is_empty();

            let y_range = self.viewport.view_rect.y_range();
            // Vertical  scale does not change often. Doing it conditionally to optimize a bit.
            if has_version_changed || meshes.viewport.view_rect.y_range() != y_range || is_animating
            {
                meshes.scaled_y.clone_from(&meshes.default);
                meshes.scaled_y.append(animated);

                // FIXME Adjust vertical note alignment. Refactor key_line_ys.
                // TODO Cleanup lanes calculation, see also key_line_ys which is duplicated here.
                for v in &mut meshes.scaled_y.vertices {
                    v.pos.y = emath::remap(
                        v.pos.y,
                        Rangef::new(
                            0.0,
                            Viewport::DEFAULT_HALF_TONE_STEP * STAVE_KEY_LANES.len() as f32,
                        ),
                        Rangef::new(
                            y_range.min + half_tone_step / 2.0,
                            y_range.max - half_tone_step / 2.0,
                        ),
                    )
                }
                meshes.viewport.view_rect.set_top(y_range.min);
                meshes.viewport.view_rect.set_bottom(y_range.max);
            }

            let has_viewport_changed = meshes.viewport != self.viewport;
            if has_version_changed || has_viewport_changed || is_animating {
                let mut mesh = Mesh::default();
                mesh.clone_from(&meshes.scaled_y);

                for v in &mut mesh.vertices {
                    v.pos.x = self.viewport.x_from_default(v.pos.x);
                }
                meshes.viewport = self.viewport.clone();
                debug_assert_eq!(meshes.viewport.view_rect, self.viewport.view_rect);
                meshes.out = Arc::new(mesh);
            }
        });
        painter.add(Shape::mesh(self.meshes.read().out.to_owned()));

        if let Some(&pointer_pos) = pointer_pos.as_ref() {
            // Hover
            let meshes = self.meshes.read();
            for triangle in 0..meshes.out_events.len() {
                if point_inside_mesh_triangle(&meshes.out, triangle, pointer_pos) {
                    *note_hovered = Some(meshes.out_events[triangle]);

                    // Hover temporary stub:
                    painter.circle_filled(pointer_pos, 10.0, COLOR_HOVERED);
                    // TODO Reimplement hovered note highlighting
                    // Bug: probably hover overflows f32 in time calculations somewhere.
                    //      Hovers are found only at the beginning of the track.
                    // painter.rect_stroke(
                    //     r,
                    //     CornerRadius::ZERO,
                    //     Stroke::new(2.0, COLOR_HOVERED),
                    //     StrokeKind::Inside,
                    // );
                    break;
                }

                // TODO Restore selection hints display.
                // if self.note_selection.contains(&event) {
                //     if x_range.max < self.x_from_time(event.at) {
                //         selection_hints_right.insert(note.pitch);
                //     } else if self.x_from_time(event.at + note.duration) < x_range.min {
                //         selection_hints_left.insert(note.pitch);
                //     }
                // }
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
                .expect("Read stave.history.")
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
                .expect("Read stave.history.")
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
            diff.clone().map(|diff| (diff.0 .0, diff.1)),
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

    fn note_animation(&self, action: &EventAction) -> Option<(Shape, Shape)> {
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
                self.paint_track_note_unscaled2(id_a, at_a, a, None),
                self.paint_track_note_unscaled2(id_b, at_b, b, None),
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
                self.paint_track_note_unscaled2(id_b, at_b, b, Some(Color32::TRANSPARENT)),
                self.paint_track_note_unscaled2(id_b, at_b, b, None),
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
                self.paint_track_note_unscaled2(id_a, at_a, a, None),
                self.paint_track_note_unscaled2(id_a, at_a, a, Some(Color32::TRANSPARENT)),
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

    fn lane_y_unscaled(lane: Pitch) -> Pix {
        (PIANO_KEY_COUNT + PIANO_LOWEST_KEY - lane) as Pix * Viewport::DEFAULT_HALF_TONE_STEP
    }

    // TODO (cleanup) Remove now redundant painting procedures
    fn track_note_shape_default(&self, event: &TrackEvent, note: &Note) -> Shape {
        Self::note_shape_unscaled(
            (event.at, event.at + note.duration),
            Self::lane_y_unscaled(note.pitch),
            Viewport::DEFAULT_HALF_TONE_STEP,
            self.note_color(&note.velocity, self.note_selection.contains(&event.id)),
        )
    }

    fn paint_track_note_unscaled2(
        &self,
        event_id: &EventId,
        at: &Time,
        note: &Note,
        color: Option<Color32>,
    ) -> Shape {
        Self::note_shape_unscaled(
            (*at, at + note.duration),
            (PIANO_KEY_COUNT + PIANO_LOWEST_KEY - note.pitch) as Pix
                * Viewport::DEFAULT_HALF_TONE_STEP,
            Viewport::DEFAULT_HALF_TONE_STEP,
            color.unwrap_or(
                self.note_color(&note.velocity, self.note_selection.contains(&event_id)),
            ),
        )
    }

    fn note_shape_unscaled(time_range: (Time, Time), y: Pix, height: Pix, color: Color32) -> Shape {
        let paint_rect = Rect {
            min: Pos2 {
                x: (time_range.0 / Viewport::DEFAULT_TIME_SCALE_DBG_INV) as f32,
                y: y - height * 0.45,
            },
            max: Pos2 {
                x: (time_range.1 / Viewport::DEFAULT_TIME_SCALE_DBG_INV) as f32,
                y: y + height * 0.45,
            },
        };
        Shape::Rect(RectShape::filled(paint_rect, CornerRadius::ZERO, color))
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

    fn point_accent_shape_default(time: Time, y: Pix, height: Pix, color: Color32) -> Shape {
        Shape::circle_filled(
            Pos2 {
                x: time as f32 * Viewport::DEFAULT_TIME_SCALE,
                y,
            },
            height / 2.2,
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

    fn cc_animation(&self, action: &EventAction) -> Option<(Shape, Shape)> {
        match (action.before(), action.after()) {
            (
                Some(TrackEvent {
                    id: id_a,
                    at: at_a,
                    event: TrackEventType::Controller(a),
                }),
                Some(TrackEvent {
                    id: id_b,
                    at: at_b,
                    event: TrackEventType::Controller(b),
                }),
            ) => Some((
                Self::point_accent_shape_default(
                    *at_a,
                    Self::lane_y_unscaled(PIANO_DAMPER_LANE),
                    Viewport::DEFAULT_HALF_TONE_STEP,
                    self.note_color(&a.value, false),
                ),
                Self::point_accent_shape_default(
                    *at_b,
                    Self::lane_y_unscaled(PIANO_DAMPER_LANE),
                    Viewport::DEFAULT_HALF_TONE_STEP,
                    self.note_color(&b.value, false),
                ),
            )),
            // New CC value
            (
                None,
                Some(TrackEvent {
                    id: id_b,
                    at: at_b,
                    event: TrackEventType::Controller(b),
                }),
            ) => Some((
                Self::point_accent_shape_default(
                    *at_b,
                    Self::lane_y_unscaled(PIANO_DAMPER_LANE),
                    Viewport::DEFAULT_HALF_TONE_STEP,
                    self.note_color(&b.value, false),
                ),
                Self::point_accent_shape_default(
                    *at_b,
                    Self::lane_y_unscaled(PIANO_DAMPER_LANE),
                    Viewport::DEFAULT_HALF_TONE_STEP,
                    self.note_color(&b.value, false),
                ),
            )),
            // Deleted value
            (
                Some(TrackEvent {
                    id: id_a,
                    at: at_a,
                    event: TrackEventType::Controller(a),
                }),
                None,
            ) => Some((
                Self::point_accent_shape_default(
                    *at_a,
                    Self::lane_y_unscaled(PIANO_DAMPER_LANE),
                    Viewport::DEFAULT_HALF_TONE_STEP,
                    self.note_color(&a.value, false),
                ),
                Self::point_accent_shape_default(
                    *at_a,
                    Self::lane_y_unscaled(PIANO_DAMPER_LANE),
                    Viewport::DEFAULT_HALF_TONE_STEP,
                    self.note_color(&a.value, false),
                ),
            )),
            _ => None,
        }
    }

    fn draw_track_cc(
        &self,
        key_ys: &BTreeMap<Pitch, Pix>,
        half_tone_step: &Pix,
        last_damper_value: &mut (Time, Level),
        event: &TrackEvent,
        cc: &ControllerSetValue,
    ) -> Option<Shape> {
        if cc.controller_id == MIDI_CC_SUSTAIN_ID {
            if let Some(y) = key_ys.get(&PIANO_DAMPER_LANE) {
                let shape = self.paint_note(
                    (last_damper_value.0, event.at),
                    *y,
                    *half_tone_step,
                    self.note_color(&last_damper_value.1, false),
                );
                *last_damper_value = (event.at, cc.value);
                return Some(shape);
            }
        }
        None
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
        .min_by_key(|(_, &y)| OrderedFloat((y - pointer_pos.y).abs()))
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

#[inline]
fn point_inside_mesh_triangle(mesh: &Mesh, triangle_idx: usize, p: Pos2) -> bool {
    let idx = triangle_idx * 3;
    let a = mesh.vertices[mesh.indices[idx] as usize].pos;
    let b = mesh.vertices[mesh.indices[idx + 1] as usize].pos;
    let c = mesh.vertices[mesh.indices[idx + 2] as usize].pos;
    point_in_triangle(p, a, b, c)
}

#[cfg(test)]
mod tests {}

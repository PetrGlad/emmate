use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ops::{Range, RangeInclusive};
use std::sync::Arc;

use eframe::egui::{
    self, Color32, Frame, Margin, Painter, PointerButton, Pos2, Rect, Response, Rounding, Sense,
    Stroke, Ui,
};
use egui::Rgba;
use ordered_float::OrderedFloat;

use crate::track::{Lane, LaneEvent, LaneEventType, Level, Note, Pitch};
use crate::Pix;

pub type StaveTime = i64;

/* Could not get a better idea yet. Having rectangles with normalized dimensions in the view models.
These can be used both to draw notes in view functions, and as means to determine
clicks and selection/drag state of a note.
Time step is 1 uSec, vertical half tone step is 1. Bottom-left 0,0 is origin.
Control events in negative y coords, 1-width bands each. */

#[derive(Debug)]
pub struct NoteView {
    rect: Rect,
    selected: bool,
}

#[derive(Debug)]
pub struct ControllerView {
    // Stub: have to see how to better represent these values.
}

// Does it make sense now to use a dyn Trait instead?
#[derive(Debug)]
pub enum EventView {
    Note(NoteView),
    Controller(ControllerView),
}

impl From<&LaneEvent> for EventView {
    fn from(event: &LaneEvent) -> Self {
        match &event.event {
            LaneEventType::Note(n) => EventView::Note(NoteView {
                rect: Self::note_rect(event.at.as_micros() as StaveTime, n),
                selected: false,
            }),
            LaneEventType::Controller(_) => EventView::Controller(ControllerView {}),
        }
    }
}

impl EventView {
    pub fn note_rect(
        at: StaveTime,
        Note {
            pitch, duration, ..
        }: &Note,
    ) -> Rect {
        let y = *pitch as Pix + 0.5;
        let x_end = (at + duration.as_micros() as StaveTime) as Pix;
        Rect {
            min: Pos2 {
                x: at as Pix,
                y: y - 0.5,
            },
            max: Pos2 {
                x: x_end,
                y: y + 0.5,
            },
        }
    }
}

#[derive(Debug)]
pub struct TrackView {
    events: Vec<EventView>,
    version: u64,
}

impl PartialEq<Self> for TrackView {
    fn eq(&self, other: &Self) -> bool {
        // Intended to check if updates are needed GUI.
        self.version == other.version
    }
}

impl From<&Lane> for TrackView {
    fn from(lane: &Lane) -> Self {
        TrackView {
            events: lane.events.iter().map(|ev| ev.into()).collect(),
            version: 0,
        }
    }
}

// Tone 60 is C3, tones start at C-2
const PIANO_KEYS: Range<Pitch> = 21..(21 + 88);

fn key_line_ys(
    view_y_range: RangeInclusive<Pix>,
    pitches: Range<Pitch>,
) -> (BTreeMap<Pitch, Pix>, Pix) {
    let mut lines = BTreeMap::new();
    let step = (view_y_range.end() - view_y_range.start()) / pitches.len() as Pix;
    let mut y = view_y_range.end() - step / 2.0;
    for p in pitches {
        lines.insert(p, y);
        y -= step;
    }
    (lines, step)
}

#[derive(Debug)]
pub struct Stave {
    pub track: Arc<Box<Lane>>,
    pub track_view_model: TrackView,
    pub time_left: StaveTime,
    pub time_right: StaveTime,
    pub view_rect: Rect,
    pub cursor_position: StaveTime,
}

impl PartialEq for Stave {
    fn eq(&self, other: &Self) -> bool {
        // TODO Want this eq implementation so egui knows when to not re-render.
        //   but comparing stave every time will be expensive. Need an optimization for that.
        //   Not comparing Lane for now, but this will cause outdated view when the notes change.
        self.track_view_model == other.track_view_model
            && self.time_left == other.time_left
            && self.time_right == other.time_right
            && self.cursor_position == other.cursor_position
            && self.view_rect == other.view_rect
    }
}

impl Stave {
    pub fn new(track: Arc<Box<Lane>>) -> Stave {
        Stave {
            track: track.clone(),
            track_view_model: TrackView::from(track.as_ref().as_ref()),
            time_left: 0,
            time_right: 300_000_000,
            view_rect: Rect::NOTHING,
            cursor_position: 0,
        }
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

    pub fn view(&mut self, ui: &mut Ui) -> Response {
        Frame::none()
            .inner_margin(Margin::symmetric(4.0, 4.0))
            .stroke(Stroke::NONE)
            .show(ui, |ui| {
                let bounds = ui.available_rect_before_wrap();
                self.view_rect = bounds;
                let (key_ys, half_tone_step) = key_line_ys(bounds.y_range(), PIANO_KEYS);
                let mut pitch_hovered = None;
                let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                if let Some(pointer_pos) = pointer_pos {
                    pitch_hovered = Some(closest_pitch(&key_ys, pointer_pos));
                    println!("Pitch hovered {:?}", pitch_hovered);
                }

                // TODO Implement note selection
                // let clicked = ui.input(|i| i.pointer.button_clicked(PointerButton::Primary));
                // if clicked && pointer_pos.is_some() && note_rect.contains(pointer_pos.unwrap())
                // {
                //     println!("Click {:?}", n);
                // }

                let painter = ui.painter_at(bounds);
                Self::draw_grid(&painter, bounds, &key_ys, &pitch_hovered);
                assert_eq!(
                    &self.track_view_model.events.len(),
                    &self.track.events.len()
                );
                for i in 0..self.track.events.len() {
                    let event = &self.track.events[i];
                    let event_view = &self.track_view_model.events[i];
                    match &event.event {
                        LaneEventType::Note(note) => {
                            let EventView::Note(note_view) = event_view else {
                                panic!("Mismatched view of an event {:?}", event_view);
                            };
                            if let Some(y) = key_ys.get(&note.pitch) {
                                self.draw_note(&painter, note, note_view, y, half_tone_step);
                            }
                        }
                        _ => (), /*println!("Not displaying event {:?}, unsupported type.", event)*/
                    }
                }

                self.draw_cursor(
                    &painter,
                    self.x_from_time(self.cursor_position),
                    Rgba::from_rgba_unmultiplied(0.1, 0.7, 0.1, 0.7).into(),
                );

                ui.allocate_response(bounds.size(), Sense::click())
            })
            .inner
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
        Note { velocity, .. }: &Note,
        event_view: &NoteView,
        y: &Pix,
        height: Pix,
    ) {
        let h = event_view.rect.height() * height;
        let paint_rect = Rect {
            min: Pos2 {
                x: self.x_from_time(event_view.rect.min.x as StaveTime),
                y: y - h * 0.45,
            },
            max: Pos2 {
                x: self.x_from_time(event_view.rect.max.x as StaveTime),
                y: y + h * 0.45,
            },
        };
        let stroke_color = note_color(&velocity, event_view.selected);
        painter.rect(paint_rect, Rounding::none(), stroke_color, Stroke::NONE);
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
                    color = Rgba::from_rgb(0.1, 0.3, 0.4)
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
        Rgba::from_rgba_unmultiplied(0.3, 0.4, 0.7, 0.5)
    } else {
        Rgba::from_rgb(0.4, 0.5, 0.5)
    };
    egui::lerp(c..=Rgba::from_rgb(0.0, 0.0, 0.0), *velocity as f32 / 128.0).into()
}

// Could not find a simple library for this.
fn ranges_intersect<T: Ord>(from_a: T, to_a: T, from_b: T, to_b: T) -> bool {
    from_a < to_b && from_b < to_a
}

use std::collections::{BTreeMap, HashMap};
use std::ops::{Range, RangeInclusive};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{
    self, Color32, Frame, Margin, Painter, Pos2, Rect, Response, Rounding, Sense, Stroke, Ui,
};
use egui::Rgba;
use midly::{MidiMessage, TrackEvent, TrackEventKind};

use crate::Pix;
use crate::track::{ControllerSetValue, Lane, LaneEvent, LaneEventType, Level, Note, Pitch};

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
            LaneEventType::Controller(c) => EventView::Controller(ControllerView {}),
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
    pub view_left: Pix,
    pub view_right: Pix,
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
            && self.view_left == other.view_left
            && self.view_right == other.view_right
    }
}

impl Stave {
    pub fn new(track: Arc<Box<Lane>>) -> Stave {
        Stave {
            track: track.clone(),
            track_view_model: TrackView::from(track.as_ref().as_ref()),
            time_left: 0,
            time_right: 300_000_000,
            view_left: 0.0,
            view_right: 300.0,
            cursor_position: 0,
        }
    }

    /// Pixel/uSec, can be cached.
    pub fn time_scale(&self) -> f32 {
        (self.view_right - self.view_left) / (self.time_right - self.time_left) as f32
    }

    pub fn x_from_time(&self, at: StaveTime) -> Pix {
        self.view_left + (at as f32 - self.time_left as f32) * self.time_scale()
    }

    pub fn time_from_x(&self, x: Pix) -> StaveTime {
        self.time_left + ((x - self.view_left) / self.time_scale()) as StaveTime
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
                self.view_left = bounds.min.x;
                self.view_right = bounds.max.x;
                assert_eq!(PIANO_KEYS.len(), 88);
                let (key_ys, half_tone_step) = key_line_ys(bounds.y_range(), PIANO_KEYS);
                let painter = ui.painter_at(bounds);

                Self::draw_grid(&painter, bounds, &key_ys);

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

                            // {
                            //     // ///////////////// Notes/time selection prototype /////////////
                            //     let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                            //     let clicked = ui.input(|i| i.pointer.button_clicked(Primary));
                            //     if clicked
                            //         && pointer_pos.is_some()
                            //         && note_rect.contains(pointer_pos.unwrap())
                            //     {
                            //         println!("Click {:?}", n);
                            //     }
                            // }
                        }
                        _ => (), /*println!("Not displaying event {:?}, unsupported type.", event)*/
                    }
                }

                self.draw_cursor(
                    &painter,
                    self.x_from_time(self.cursor_position),
                    Rgba::from_rgba_unmultiplied(0.1, 0.8, 0.1, 0.8).into(),
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
        half_tone_step: Pix,
    ) {
        let h = event_view.rect.height() * half_tone_step;
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

        //
        // let x_end = x + (duration.as_micros() as f32 * time_step);
        // let stroke_width = half_tone_step * 0.9;
        //
        // painter.hline(
        //     x..=x_end,
        //     y,
        //     Stroke {
        //         width: stroke_width,
        //         color: stroke_color,
        //     },
        // );
        // let half_width = stroke_width / 2.0;
        // painter.circle_filled(Pos2::new(x, y), half_width, stroke_color);
        // painter.circle_filled(Pos2::new(x_end, y), half_width, stroke_color);
    }

    fn draw_grid(painter: &Painter, bounds: Rect, keys: &BTreeMap<Pitch, Pix>) {
        let is_black_key = |tone: &Pitch| vec![1, 3, 6, 8, 10].contains(&(tone % 12));
        let mut i = 0;
        for (pitch, y) in keys {
            let color = if is_black_key(&pitch) {
                Rgba::from_rgb(0.05, 0.05, 0.05)
            } else {
                Rgba::from_rgb(0.55, 0.55, 0.55)
            };
            painter.hline(
                bounds.min.x..=bounds.max.x,
                *y,
                Stroke {
                    width: 1.0,
                    color: color.into(),
                },
            );
            i += 1;
        }
    }
}

fn note_color(velocity: &Level, selected: bool) -> Color32 {
    let c = if selected {
        Rgba::from_rgba_unmultiplied(0.3, 0.4, 0.7, 0.5)
    } else {
        Rgba::from_rgb(0.4, 0.5, 0.5)
    };
    egui::lerp(c..=Rgba::from_rgb(0.0, 0.0, 0.0), *velocity as f32 / 128.0).into()
}

pub fn to_lane_events(events: Vec<TrackEvent<'static>>, tick_duration: u64) -> Vec<LaneEvent> {
    // TODO The offset calculations are very similar to ones in the engine. Can these be shared.
    let mut ons: HashMap<Pitch, (u64, MidiMessage)> = HashMap::new();
    let mut lane_events = vec![];
    let mut at: u64 = 0;
    for ev in events {
        at += ev.delta.as_int() as u64 * tick_duration;
        match ev.kind {
            TrackEventKind::Midi { message, .. } => match message {
                MidiMessage::NoteOn { key, .. } => {
                    ons.insert(key.as_int() as Pitch, (at, message));
                }
                MidiMessage::NoteOff { key, .. } => {
                    let on = ons.remove(&(key.as_int() as Pitch));
                    match on {
                        Some((t, MidiMessage::NoteOn { key, vel })) => {
                            lane_events.push(LaneEvent {
                                at: Duration::from_micros(t),
                                event: LaneEventType::Note(Note {
                                    duration: Duration::from_micros(at - t),
                                    pitch: key.as_int() as Pitch,
                                    velocity: vel.as_int() as Level,
                                }),
                            });
                        }
                        None => eprintln!("INFO NoteOff event without NoteOn {:?}", ev),
                        _ => panic!("ERROR Unexpected state: {:?} event in \"on\" queue.", on),
                    }
                }
                MidiMessage::Controller { controller, value } => lane_events.push(LaneEvent {
                    at: Duration::from_micros(at),
                    event: LaneEventType::Controller(ControllerSetValue {
                        controller_id: controller.into(),
                        value: value.into(),
                    }),
                }),
                _ => eprintln!("DEBUG Event ignored {:?}", ev),
            },
            _ => (),
        };
    }
    // Notes are collected after they complete, This mixes the ordering with immediate events.
    lane_events.sort_by_key(|ev| ev.at.as_micros());
    lane_events
}

// Could not find a simple library for this.
fn ranges_intersect<T: Ord>(from_a: T, to_a: T, from_b: T, to_b: T) -> bool {
    from_a < to_b && from_b < to_a
}

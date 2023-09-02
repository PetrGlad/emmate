use alsa::card::Iter;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::PointerButton::Primary;
use eframe::egui::{self, Color32, Frame, Margin, Painter, Pos2, Rect, Response, Rounding, Sense, Stroke, Ui};
use eframe::emath::Vec2;
use egui::Rgba;
use midly::{MidiMessage, TrackEvent, TrackEventKind};

use crate::track::{ControllerSetValue, Lane, LaneEvent, LaneEventType, Level, Note, Pitch};
use crate::Pix;

pub type StaveTime = i64;

/* Could not get a better idea yet. Having rectangles with normalized dimensions in the view models.
These can be used both to draw notes in view functions, and as means to determine
clicks and selection/drag state of a note.
Time step is 1 uSec, vertical half tone step is 1. Bottom-left 0,0 is origin.
Control events in negative y coords, 1-width bands each. */
#[derive(Debug)]
pub struct EventViewModel {
    rect: Rect,
    selected: bool,
}

impl From<&LaneEvent> for EventViewModel {
    fn from(event: &LaneEvent) -> Self {
        match &event.event {
            LaneEventType::Note(n) => EventViewModel {
                rect: Self::note_rect(event.at.as_micros() as StaveTime, n),
                selected: false,
            },
            LaneEventType::Controller(c) => EventViewModel {
                rect: Self::controller_value_rect(event.at.as_micros() as StaveTime, c),
                selected: false,
            },
        }
    }
}

impl EventViewModel {
    pub fn controller_value_rect(
        at: StaveTime,
        ControllerSetValue { value, .. }: &ControllerSetValue,
    ) -> Rect {
        let y = *value as Pix / Level::MAX as Pix - 0.5;
        // Stub: see how to represent these values.
        Rect {
            min: Pos2 {
                x: at as Pix,
                y: y - 0.5,
            },
            max: Pos2 {
                x: at as Pix,
                y: y + 0.5,
            },
        }
    }

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
pub struct TrackViewModel {
    notes: Vec<EventViewModel>,
    version: u64,
}

impl PartialEq<Self> for TrackViewModel {
    fn eq(&self, other: &Self) -> bool {
        // Intended to check if updates are needed GUI.
        self.version == other.version
    }
}

impl From<&Lane> for TrackViewModel {
    fn from(lane: &Lane) -> Self {
        TrackViewModel {
            notes: lane.events.iter().map(|ev| ev.into()).collect(),
            version: 0,
        }
    }
}

#[derive(Debug)]
pub struct Stave {
    pub track: Arc<Box<Lane>>,
    pub track_view_model: TrackViewModel,
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
            track_view_model: TrackViewModel::from(track.as_ref().as_ref()),
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
                let key_count = 88 as Pitch;
                // Tone 60 is C3, tones start at C-2
                let first_key = 21 as Pitch;
                let half_tone_step = bounds.height() / key_count as f32;
                let bottom_line = bounds.max.y - half_tone_step / 2.0;
                let painter = ui.painter_at(bounds);

                Self::draw_grid(
                    &painter,
                    bounds,
                    key_count,
                    &first_key,
                    half_tone_step,
                    bottom_line,
                );

                assert_eq!(&self.track_view_model.notes.len(), &self.track.events.len());
                for i in 0..self.track.events.len() {
                    let event = &self.track.events[i];
                    let event_view = &self.track_view_model.notes[i];
                    match &event.event {
                        LaneEventType::Note(note) => {
                            Self::draw_note(
                                self,
                                &painter,
                                first_key,
                                &half_tone_step,
                                self.time_scale(),
                                bottom_line,
                                note,
                                event_view,
                            );

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
        first_key: Pitch,
        half_tone_step: &f32,
        time_step: f32,
        bottom_line: Pix,
        Note {
            pitch,
            velocity,
            duration,
        }: &Note,
        event_view: &EventViewModel,
    ) {
        let paint_rect = Rect {
            min: Pos2 {
                x: self.x_from_time(event_view.rect.min.x as StaveTime),
                y: event_view.rect.min.y * half_tone_step,
            },
            max: Pos2 {
                x: self.x_from_time(event_view.rect.max.x as StaveTime),
                y: event_view.rect.max.y * half_tone_step,
            },
        };
        let stroke_color = note_color(&velocity, event_view.selected);
        painter.rect(paint_rect, Rounding::none(), stroke_color,Stroke::NONE);

        // let y = bottom_line - half_tone_step * (pitch - first_key) as Pix;
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

    fn draw_grid(
        painter: &Painter,
        bounds: Rect,
        key_count: Pitch,
        first_key: &Pitch,
        tone_step: Pix,
        bottom_line: Pix,
    ) {
        let is_black_key = |tone: &Pitch| vec![1, 3, 6, 8, 10].contains(&(tone % 12));
        for key in 0..key_count {
            let color = if is_black_key(&(first_key + key)) {
                Rgba::from_rgb(0.05, 0.05, 0.05)
            } else {
                Rgba::from_rgb(0.55, 0.55, 0.55)
            };
            let y = bottom_line - tone_step * key as Pix;
            painter.hline(
                bounds.min.x..=bounds.max.x,
                y,
                Stroke {
                    width: 1.0,
                    color: color.into(),
                },
            );
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

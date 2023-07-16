use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::engine::TransportTime;
use eframe::egui::{self, Color32, Frame, Margin, Painter, Pos2, Rect, Response, Sense, Stroke, Ui};
use egui::Rgba;
use midly::{MidiMessage, TrackEvent, TrackEventKind};

use crate::track::{ControllerSetValue, Lane, LaneEvent, LaneEventType, Level, Note, Pitch};

#[derive(Debug)]
pub struct Stave {
    pub track: Arc<Box<Lane>>,
    /// Pixel/uSec
    pub time_scale: f32,
    pub viewport_left: TransportTime,
    pub cursor_position: TransportTime,
}

impl PartialEq for Stave {
    fn eq(&self, other: &Self) -> bool {
        // TODO Want this eq implementation so egui knows when not to re-render.
        //   but comparing stave every time will be expensive. Need an optimization for that.
        //   Not comparing Lane for now, but this will cause outdated view when the notes change.
        self.time_scale == other.time_scale
            && self.viewport_left == other.viewport_left
            && self.cursor_position == other.cursor_position
    }
}

impl Stave {
    pub fn view(&mut self, ui: &mut Ui) -> Response {
        Frame::none()
            .inner_margin(Margin::symmetric(4.0, 4.0))
            .stroke(Stroke::NONE)
            .show(ui, |ui| {
                let bounds = ui.available_rect_before_wrap();
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

                let time_to_x =
                    |at| bounds.min.x + (at as f32 - self.viewport_left as f32) * &self.time_scale;
                for LaneEvent { at, event } in &self.track.events {
                    let x = time_to_x(at.as_micros() as u64);
                    match event {
                        LaneEventType::Note(n) => {
                            Self::draw_note(
                                &painter,
                                first_key,
                                &half_tone_step,
                                self.time_scale,
                                bottom_line,
                                x,
                                n,
                            );
                        }
                        _ => () /*println!("Not displaying event {:?}, unsupported type.", event)*/,
                    }
                }

                self.draw_cursor(&painter, time_to_x(self.cursor_position));

                ui.allocate_response(bounds.size(), Sense::hover())
            }).inner
    }

    fn draw_cursor(&self, painter: &Painter, x: f32) {
        painter.vline(
            x,
            painter.clip_rect().y_range(),
            Stroke {
                width: 2.0,
                color: Rgba::from_rgba_unmultiplied(0.1, 0.8, 0.1, 0.8).into(),
            },
        )
    }

    fn draw_note(
        painter: &Painter,
        first_key: Pitch,
        half_tone_step: &f32,
        time_step: f32,
        bottom_line: f32,
        x: f32,
        Note {
            pitch,
            velocity,
            duration,
        }: &Note,
    ) {
        let y = bottom_line - half_tone_step * (pitch - first_key) as f32;
        let x_end = x + (duration.as_micros() as f32 * time_step);
        let stroke_width = half_tone_step * 0.9;
        let stroke_color = note_color(&velocity);
        painter.hline(
            x..=x_end,
            y,
            Stroke {
                width: stroke_width,
                color: stroke_color,
            },
        );
        painter.circle_filled(Pos2::new(x, y), stroke_width / 2.0, stroke_color);
        painter.circle_filled(Pos2::new(x_end, y), stroke_width / 2.0, stroke_color);
    }

    fn draw_grid(
        painter: &Painter,
        bounds: Rect,
        key_count: Pitch,
        first_key: &Pitch,
        tone_step: f32,
        bottom_line: f32,
    ) {
        let is_black_key = |tone: &Pitch| vec![1, 3, 6, 8, 10].contains(&(tone % 12));
        for key in 0..key_count {
            let color = if is_black_key(&(first_key + key)) {
                Rgba::from_rgb(0.05, 0.05, 0.05)
            } else {
                Rgba::from_rgb(0.55, 0.55, 0.55)
            };
            let y = bottom_line - tone_step * key as f32;
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

fn note_color(velocity: &Level) -> Color32 {
    egui::lerp(
        Rgba::from_rgb(0.4, 0.5, 0.5)..=Rgba::from_rgb(0.0, 0.0, 0.0),
        *velocity as f32 / 128.0,
    )
    .into()
}

pub fn to_lane_events(events: Vec<TrackEvent<'static>>, tick_duration: u64) -> Vec<LaneEvent> {
    // TODO Think if we should use Note in the engine also - the calculations are very similar.
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

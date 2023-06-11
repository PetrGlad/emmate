use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::canvas::{Cursor, Frame, Geometry, LineCap, Path, Stroke};
use iced::widget::{canvas, Canvas};
use iced::{Color, Element, Length, Point, Rectangle, Theme};
use midly::{MidiMessage, TrackEvent, TrackEventKind};
use palette::Blend;
use palette::Srgba;

use crate::track::{ControllerSetValue, Lane, LaneEvent, LaneEventType, Level, Note, Pitch};

#[derive(Debug, Default)]
pub struct Stave {
    // Pixel/uSec
    pub time_scale: f32,
    pub track: Arc<Box<Lane>>,
}

impl Stave {
    pub fn view(&self) -> Element<()> {
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn draw_grid(
        frame: &mut Frame,
        key_count: Pitch,
        first_key: &Pitch,
        tone_step: f32,
        bottom_line: f32,
    ) {
        // Grid
        let black_key = |tone: &Pitch| vec![1, 3, 6, 8, 10].contains(&(tone % 12));
        for key in 0..key_count {
            let color = if black_key(&(first_key + key)) {
                Color::from_rgb(0.1, 0.1, 0.1)
            } else {
                Color::from_rgb(0.9, 0.9, 0.9)
            };
            let y = bottom_line - tone_step * key as f32;
            frame.stroke(
                &Path::line(
                    Point { x: 0.0, y },
                    Point {
                        x: frame.width(),
                        y,
                    },
                ),
                Stroke::default().with_color(color),
            );
        }
    }
}

fn note_color(velocity: &Level) -> Color {
    let ratio = 1.0 - velocity.clone() as f32 / 128.0;
    let slow = Color::from_rgb(0.8 * &ratio, 0.8 * &ratio, 0.9 * &ratio);
    let fast = Color::from_rgb(0.02, 0.02, 0.02);
    let l1 = Srgba::from(slow).into_linear();
    let l2 = Srgba::from(fast).into_linear();
    Color::from(Srgba::from_linear(l1.lighten(l2)))
}

impl canvas::Program<()> for Stave {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let key_count = 88 as Pitch;
        // Tone 60 is C3, tones start at C-2
        let first_key = 21 as Pitch;
        let tone_step = bounds.height / key_count as f32;
        let bottom_line = key_count as f32 * tone_step;
        let time_step = bounds.width * &self.time_scale;
        let mut frame = Frame::new(bounds.size());

        Self::draw_grid(&mut frame, key_count, &first_key, tone_step, bottom_line);

        // Notes
        for LaneEvent { at, event } in &self.track.events {
            let x = at.as_micros() as f32 * time_step;
            match event {
                LaneEventType::Note(Note {
                    pitch,
                    velocity,
                    duration,
                }) => {
                    let y = bottom_line - tone_step * (pitch - first_key) as f32;
                    frame.stroke(
                        &Path::line(
                            Point { x, y },
                            Point {
                                x: x + (duration.as_micros() as f32 * time_step),
                                y,
                            },
                        ),
                        Stroke::default()
                            .with_color(note_color(&velocity))
                            .with_width(&tone_step * 0.9)
                            .with_line_cap(LineCap::Round),
                    );
                }
                _ => println!("Event {:?} not supported yet.", event),
            }
        }

        // Cursor
        frame.stroke(
            &Path::line(
                Point { x: 100.0, y: 0.0 },
                Point {
                    x: 100.0,
                    y: frame.height(),
                },
            ),
            Stroke::default()
                .with_color(Color { r: 0.1, g: 0.8, b: 0.1, a: 0.7 })
                .with_width(3.0)
                .with_line_cap(LineCap::Square),
        );

        // let background = Path::rectangle(Point::ORIGIN, frame.size());
        // frame.fill(&background, Color::WHITE);

        // let circle = Path::circle(frame.center(), self.time_scale.into());
        // frame.fill(&circle, Color::BLACK);
        vec![frame.into_geometry()]
    }
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

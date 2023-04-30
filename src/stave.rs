use std::collections::{HashMap, HashSet};
use std::time::Duration;
use iced::{Color, Element, Length, Point, Rectangle, Theme};
use iced::widget::{canvas, Canvas};
use iced::widget::canvas::{Cursor, Frame, Geometry, LineCap, Path, Stroke};
use midly::{MidiMessage, TrackEvent, TrackEventKind};
use palette::Srgba;
use palette::Blend;

type Pitch = u8;
type Velocity = u8;

#[derive(Debug)]
pub struct Note {
    pub pitch: Pitch,
    pub velocity: Velocity,
    // Since the track beginning.
    pub on: Duration,
    pub duration: Duration,
}

#[derive(Debug, Default)]
pub struct Stave {
    // Pixel/uSec
    pub time_scale: f32,
    pub notes: Vec<Note>,
}

impl Stave {
    pub fn view(&self) -> Element<()> {
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn note_color(velocity: &Velocity) -> Color {
    let coeff = 1.0 - velocity.clone() as f32 / 128.0;
    let slow = Color::from_rgb(0.8 * &coeff, 0.9 * &coeff, 0.9 * &coeff);
    let fast = Color::from_rgb(0.03, 0.03, 0.03);
    let l1 = Srgba::from(slow).into_linear();
    let l2 = Srgba::from(fast).into_linear();
    Color::from(Srgba::from_linear(l1.lighten(l2)))
}

impl canvas::Program<()> for Stave {
    type State = ();

    fn draw(&self, _state: &Self::State, _theme: &Theme, bounds: Rectangle, _cursor: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(bounds.size());
        let key_count = 88;
        let tone_step = bounds.height / key_count as f32;
        let mut bottom_line = 0.0;

        // Grid
        let black_key = |tone: &i32| vec![1, 4, 6, 9, 11].contains(&(*tone % 12));
        for row in 0..key_count {
            let color = if black_key(&row) {
                Color::from_rgb(0.3, 0.3, 0.3)
            } else {
                Color::from_rgb(0.9, 0.9, 0.9)
            };
            let y = &tone_step * row.to_owned() as f32;
            bottom_line = y;
            frame.stroke(&Path::line(Point { x: 0.0, y },
                                     Point { x: frame.width(), y }),
                         Stroke::default().with_color(color));
        }

        // Notes
        let time_step = bounds.width * &self.time_scale;
        for Note { on, duration, pitch, velocity } in &self.notes
        {
            let y = &bottom_line - &tone_step * pitch.to_owned() as f32;
            let x = on.as_micros() as f32 * time_step;
            frame.stroke(&Path::line(Point { x, y },
                                     Point { x: x + (duration.as_micros() as f32 * time_step), y }),
                         Stroke::default()
                             .with_color(note_color(&velocity))
                             .with_width(&tone_step * 0.9)
                             .with_line_cap(LineCap::Round));
        }

        // let background = Path::rectangle(Point::ORIGIN, frame.size());
        // frame.fill(&background, Color::WHITE);

        // let circle = Path::circle(frame.center(), self.time_scale.into());
        // frame.fill(&circle, Color::BLACK);
        vec![frame.into_geometry()]
    }
}

pub fn events_to_notes(events: Vec<TrackEvent<'static>>) -> Vec<Note> {
    // TODO Think if we should use Note in the engine also - the calculations are very similar.
    let mut ons: HashMap<Pitch, (u64, MidiMessage)> = HashMap::new();
    let mut notes = vec![];
    let mut at: u64 = 0;
    for ev in events {
        at += ev.delta.as_int() as u64;
        match ev.kind {
            TrackEventKind::Midi { message, .. } => {
                match message {
                    MidiMessage::NoteOn { key, .. } => {
                        ons.insert(key.as_int() as Pitch, (at.to_owned(), message));
                    }
                    MidiMessage::NoteOff { key, .. } => {
                        let on = ons.remove(&(key.as_int() as Pitch));
                        match on {
                            Some((t, MidiMessage::NoteOn { key, vel })) => {
                                notes.push(Note {
                                    on: Duration::from_micros(t),
                                    duration: Duration::from_micros(at.to_owned() - t.to_owned()),
                                    pitch: key.as_int() as Pitch,
                                    velocity: vel.as_int() as Velocity,
                                });
                            }
                            Some(_) => (),
                            None => eprintln!("INFO NoteOff event without NoteOn {:?}", ev)
                        }
                    }
                    _ => ()
                }
            }
            _ => ()
        };
    }
    notes
}

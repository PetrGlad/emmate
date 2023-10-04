use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use eframe::egui::{
    self, Color32, Frame, Key, Margin, Painter, PointerButton, Pos2, Rangef, Rect, Response,
    Rounding, Sense, Stroke, Ui,
};
use egui::Rgba;
use ordered_float::OrderedFloat;

use crate::engine::TransportTime;
use crate::midi::serialize_smf;
use crate::track::{
    switch_cc_on, to_midi_events, Lane, LaneEvent, LaneEventType, Level, Note, Pitch,
    MIDI_CC_SUSTAIN,
};
use crate::{track, Pix};

pub type StaveTime = i64;

/* Could not get a better idea yet. Having rectangles with normalized dimensions in the view models.
These can be used both to draw notes in view functions, and as means to determine
clicks and selection/drag state of a note.
Time step is 1 uSec, vertical half tone step is 1. Bottom-left 0,0 is origin.
Control events in negative y coords, 1-width bands each. */

#[derive(Debug)]
pub struct NoteView {
    rect: Rect,
    // TODO Implement selection, maybe this could be a separate bag of notes instead.
    selected: bool,
}

#[derive(Debug)]
pub struct ControllerView {
    // Stub: have to see how to better represent these values.
}

// Does it make sense now to use a dyn Trait instead?
// Is it really needed? Likely will not be using it for selection.
#[derive(Debug)]
pub enum EventView {
    Note(NoteView),
    Controller(ControllerView),
}

impl From<&LaneEvent> for EventView {
    fn from(event: &LaneEvent) -> Self {
        match &event.event {
            LaneEventType::Note(n) => EventView::Note(NoteView {
                rect: NoteView::note_rect(event.at as StaveTime, n),
                selected: false,
            }),
            LaneEventType::Controller(_) => EventView::Controller(ControllerView {}),
        }
    }
}

impl NoteView {
    pub fn note_rect(
        at: StaveTime,
        Note {
            pitch, duration, ..
        }: &Note,
    ) -> Rect {
        let y = *pitch as Pix + 0.5;
        let x_end = (at + *duration as StaveTime) as Pix;
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

// Tone 60 is C3, tones start at C-2 (21)
const PIANO_LOWEST_KEY: Pitch = 21;
const PIANO_KEY_LINES: Range<Pitch> = PIANO_LOWEST_KEY..(PIANO_LOWEST_KEY + 88);
const PIANO_DAMPER_LINE: Pitch = PIANO_LOWEST_KEY - 1;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TimeSelection {
    pub from: StaveTime,
    pub to: StaveTime,
}

fn to_transport_time(value: StaveTime) -> TransportTime {
    value.max(0) as TransportTime
}

impl From<&TimeSelection> for track::TimeSelection {
    fn from(value: &TimeSelection) -> Self {
        track::TimeSelection {
            from: to_transport_time(value.from),
            to: to_transport_time(value.to),
        }
    }
}

#[derive(Debug)]
pub struct Stave {
    pub track: Arc<RwLock<Lane>>,
    pub time_left: StaveTime,
    pub time_right: StaveTime,
    pub view_rect: Rect,
    pub cursor_position: StaveTime,
    pub time_selection: Option<TimeSelection>,
}

impl PartialEq for Stave {
    fn eq(&self, other: &Self) -> bool {
        // This eq implementation helps so egui knows when not to re-render.
        self.time_left == other.time_left
            && self.time_right == other.time_right
            && self.cursor_position == other.cursor_position
            && self.view_rect == other.view_rect
    }
}

const COLOR_SELECTED: Rgba = Rgba::from_rgb(0.2, 0.5, 0.55);
// TODO Hovered color should be a function (takes normal color and highlights it slightly)
const COLOR_HOVERED: Rgba = COLOR_SELECTED; // Rgba::from_rgba_unmultiplied(0.3, 0.4, 0.7, 0.5);

impl Stave {
    pub fn new(track: Arc<RwLock<Lane>>) -> Stave {
        Stave {
            track: track.clone(),
            time_left: 0,
            time_right: 300_000_000,
            view_rect: Rect::NOTHING,
            cursor_position: 0,
            time_selection: None,
        }
    }

    pub fn save_to(&self, file_path: &PathBuf) {
        let usec_per_tick = 26u32;
        let midi_events = to_midi_events(
            &self.track.read().expect("Cannot read track.").events,
            usec_per_tick,
        );
        let mut binary = Vec::new();
        serialize_smf(midi_events, usec_per_tick, &mut binary)
            .expect("Cannot serialize midi track.");
        std::fs::write(&file_path, binary)
            .expect(&*format!("Cannot save to {}", &file_path.display()));
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

    // (Widget would require fn ui(self, ui: &mut Ui) -> Response)
    pub fn view(&mut self, ui: &mut Ui) -> Response {
        let response = Frame::none()
            .inner_margin(Margin::symmetric(4.0, 4.0))
            .stroke(Stroke::NONE)
            .show(ui, |ui| {
                let bounds = ui.available_rect_before_wrap();
                self.view_rect = bounds;
                let (key_ys, half_tone_step) = key_line_ys(&bounds.y_range(), PIANO_KEY_LINES);
                let mut pitch_hovered = None;
                let mut time_hovered = None;
                let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                if let Some(pointer_pos) = pointer_pos {
                    pitch_hovered = Some(closest_pitch(&key_ys, pointer_pos));
                    time_hovered = Some(self.time_from_x(pointer_pos.x));
                }

                let painter = ui.painter_at(bounds);

                // TODO Implement note selection
                // let clicked = ui.input(|i| i.pointer.button_clicked(PointerButton::Primary));
                // if clicked && pointer_pos.is_some() && note_rect.contains(pointer_pos.unwrap())
                // {
                //     println!("Click {:?}", n);
                // }

                Self::draw_grid(&painter, bounds, &key_ys, &pitch_hovered);
                if let Some(s) = &self.time_selection {
                    self.draw_time_selection(&painter, &s);
                }
                let track = self.track.read().expect("Cannot read track.");
                for i in 0..track.events.len() {
                    let event = &track.events[i];
                    let event_view = event.into();
                    match &event.event {
                        LaneEventType::Note(note) => {
                            let EventView::Note(note_view) = event_view else {
                                // TODO Move draw logic into event_view
                                panic!("Mismatched view of an event {:?}", event_view);
                            };
                            // TODO Implement note selection, here is a hover demo instead:
                            if let Some(y) = key_ys.get(&note.pitch) {
                                // Stub:
                                let selected = if let Some(t) = &time_hovered {
                                    if let Some(p) = pitch_hovered {
                                        event.is_active(*t as TransportTime) && p == note.pitch
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                };
                                let nv = {
                                    NoteView {
                                        rect: note_view.rect.clone(),
                                        selected,
                                    }
                                };

                                self.draw_note(&painter, note, &nv, y, half_tone_step);
                            }
                        }
                        LaneEventType::Controller(v) if v.controller_id == MIDI_CC_SUSTAIN => {
                            if let Some(y) = key_ys.get(&PIANO_DAMPER_LINE) {
                                self.draw_cc(
                                    &painter,
                                    event.at as StaveTime,
                                    v.value,
                                    *y,
                                    half_tone_step,
                                );
                            }
                        }
                        _ => (), /*println!(
                                     "Not displaying event {:?}, the event type is not supported yet.",
                                     event
                                 )*/
                    }
                }

                self.draw_cursor(
                    &painter,
                    self.x_from_time(self.cursor_position),
                    Rgba::from_rgba_unmultiplied(0.1, 0.7, 0.1, 0.7).into(),
                );

                ui.allocate_response(bounds.size(), Sense::click_and_drag())
            })
            .inner;

        self.update_time_selection(&response);
        self.handle_commands(&response);

        response
    }

    fn handle_commands(&mut self, response: &Response) {
        if response.ctx.input(|i| i.key_pressed(Key::Delete)) {
            if let Some(time_selection) = &self.time_selection {
                self.track
                    .write()
                    .expect("Cannot write to track.")
                    .tape_cut(&time_selection.into());
            }
        }
    }

    fn update_time_selection(&mut self, response: &Response) {
        let drag_button = PointerButton::Primary;
        let hover_pos = &response.hover_pos();
        if response.clicked_by(drag_button) {
            self.time_selection = None;
        } else if response.drag_started_by(drag_button) {
            let x = hover_pos.unwrap().x;
            let time = self.time_from_x(x);
            self.time_selection = Some(TimeSelection {
                from: time,
                to: time,
            });
        } else if response.drag_released_by(drag_button) {
            // Just documenting how it can be handled
        } else if response.dragged_by(drag_button) {
            if let Some(Pos2 { x, .. }) = hover_pos {
                let time = self.time_from_x(*x);
                let selection = self.time_selection.as_mut().unwrap();
                selection.to = time;
            }
        }
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
        painter.rect(paint_rect, Rounding::ZERO, stroke_color, Stroke::NONE);
    }

    fn draw_cc(&self, painter: &Painter, at: StaveTime, value: Level, y: Pix, height: Pix) {
        let h = height * 0.95;
        let on = switch_cc_on(value);
        painter.circle(
            Pos2::new(
                self.x_from_time(at),
                if on { y - (h / 4.0) } else { y + (h / 4.0) },
            ),
            h / 4.0,
            if on {
                Rgba::from_rgb(0.3, 0.1, 0.1)
            } else {
                Rgba::from_rgb(0.1, 0.1, 0.3)
            },
            Stroke::NONE,
        )
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

    pub fn draw_time_selection(&self, painter: &Painter, selection: &TimeSelection) {
        let clip = painter.clip_rect();
        let area = Rect {
            min: Pos2 {
                x: self.x_from_time(selection.from),
                y: clip.min.y,
            },
            max: Pos2 {
                x: self.x_from_time(selection.to),
                y: clip.max.y,
            },
        };
        let color = Color32::from_rgba_unmultiplied(64, 80, 100, 60);
        painter.rect(area, Rounding::ZERO, color, Stroke::NONE);
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
        )
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
        COLOR_SELECTED
    } else {
        Rgba::from_rgb(0.4, 0.5, 0.5)
    };
    egui::lerp(c..=Rgba::from_rgb(0.0, 0.0, 0.0), *velocity as f32 / 128.0).into()
}

// Could not find a simple library for this.
fn ranges_intersect<T: Ord>(from_a: T, to_a: T, from_b: T, to_b: T) -> bool {
    from_a < to_b && from_b < to_a
}

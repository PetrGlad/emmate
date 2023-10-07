use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::SeqCst;

use midly::num::u4;
use midly::{MidiMessage, TrackEvent, TrackEventKind};

use crate::engine::TransportTime;
use crate::midi;

pub type Pitch = u8;
pub type ControllerId = u8;
pub type Level = u8;
pub type ChannelId = u8;
pub type EventId = u64;

pub const MIDI_CC_MODWHEEL: ControllerId = 1;
// Damper pedal
pub const MIDI_CC_SUSTAIN: ControllerId = 64;

pub fn switch_cc_on(x: Level) -> bool {
    // Pianoteq seem to support continuous damper values, may support this later.
    // Not using crappy SLP3-D anyway.
    x >= 64
}

#[derive(Debug, Eq, PartialEq)]
pub struct Note {
    pub pitch: Pitch,
    pub velocity: Level,
    pub duration: TransportTime,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ControllerSetValue {
    pub controller_id: ControllerId,
    pub value: Level,
}

#[derive(Debug, Eq, PartialEq)]
pub enum LaneEventType {
    Note(Note),
    Controller(ControllerSetValue),
}

#[derive(Debug, Eq, PartialEq)]
pub struct LaneEvent {
    pub id: EventId,
    /// Since the track beginning.
    pub at: TransportTime,
    pub event: LaneEventType,
}

impl LaneEvent {
    pub fn is_active(&self, at: TransportTime) -> bool {
        match &self.event {
            LaneEventType::Note(n) => (self.at..(self.at + n.duration)).contains(&at),
            _ => false,
        }
    }
}

impl PartialOrd for LaneEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // TODO Maybe consider complete comparison (including actual events)
        //      to avoid ambiguities in sorting.
        Some(self.at.cmp(&other.at))
    }
}

impl Ord for LaneEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(&other).unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TimeSelection {
    pub from: TransportTime,
    pub to: TransportTime,
}

impl TimeSelection {
    pub fn length(&self) -> TransportTime {
        self.to - self.from
    }

    pub fn contains(&self, at: TransportTime) -> bool {
        self.from <= at && at < self.to
    }

    pub fn before(&self, at: TransportTime) -> bool {
        self.to <= at
    }
}

#[derive(Debug, Default)]
pub struct Lane {
    // Notes should always be ordered by start time ascending. Not enforced yet.
    pub events: Vec<LaneEvent>,
    version: u64,
    id_seq: AtomicU64,
}

impl Lane {
    pub fn new(events: Vec<LaneEvent>) -> Lane {
        Lane {
            events,
            version: 0,
            id_seq: AtomicU64::new(0),
        }
    }

    pub fn load_from(&mut self, file_path: &PathBuf) -> bool {
        if let Ok(data) = std::fs::read(&file_path) {
            let events = midi::load_smf(&data);
            self.events = to_lane_events(&mut self.id_seq, events.0, events.1 as u64);
            return true;
        }
        false
    }

    pub fn save_to(&self, file_path: &PathBuf) {
        let usec_per_tick = 26u32;
        let midi_events = to_midi_events(&self.events, usec_per_tick);
        let mut binary = Vec::new();
        midi::serialize_smf(midi_events, usec_per_tick, &mut binary)
            .expect("Cannot serialize midi track.");
        std::fs::write(&file_path, binary)
            .expect(&*format!("Cannot save to {}", &file_path.display()));
    }

    pub fn tape_cut(&mut self, time_selection: &TimeSelection) {
        dbg!("tape_cut", time_selection);
        self.version += 1;
        let d = time_selection.length();
        self.events.retain(|ev| !time_selection.contains(ev.at));
        for ev in &mut self.events {
            if time_selection.before(ev.at) {
                ev.at -= d;
            }
        }
    }

    pub fn delete_events(&mut self, event_ids: &HashSet<EventId>) {
        self.version += 1;
        self.events.retain(|ev| !event_ids.contains(&ev.id));
    }
}

pub fn to_lane_events(
    id_seq: &mut AtomicU64,
    events: Vec<TrackEvent<'static>>,
    tick_duration: TransportTime,
) -> Vec<LaneEvent> {
    // TODO The offset calculations are very similar to ones in the engine. Can these be shared?
    let mut ons: HashMap<Pitch, (TransportTime, MidiMessage)> = HashMap::new();
    let mut lane_events = vec![];
    let mut at: TransportTime = 0;
    for ev in events {
        at += ev.delta.as_int() as TransportTime * tick_duration;
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
                                id: id_seq.fetch_add(1, SeqCst),
                                at: t,
                                event: LaneEventType::Note(Note {
                                    duration: at - t,
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
                    id: id_seq.fetch_add(1, SeqCst),
                    at,
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
    lane_events.sort_by_key(|ev| ev.at);
    lane_events
}

/// Reverse of to_lane_events
pub fn to_midi_events(events: &Vec<LaneEvent>, usec_per_tick: u32) -> Vec<TrackEvent<'static>> {
    let channel = u4::from(0); // Channel hard coded.
    let mut buffer: Vec<(TransportTime, TrackEventKind)> = vec![];
    for ev in events {
        match &ev.event {
            LaneEventType::Note(n) => {
                buffer.push((
                    ev.at,
                    TrackEventKind::Midi {
                        channel,
                        message: MidiMessage::NoteOn {
                            key: n.pitch.into(),
                            vel: n.velocity.into(),
                        },
                    },
                ));
                buffer.push((
                    ev.at + n.duration,
                    TrackEventKind::Midi {
                        channel,
                        message: MidiMessage::NoteOff {
                            key: n.pitch.into(),
                            vel: n.velocity.into(),
                        },
                    },
                ));
            }
            LaneEventType::Controller(v) => {
                buffer.push((
                    ev.at,
                    TrackEventKind::Midi {
                        channel,
                        message: MidiMessage::Controller {
                            controller: v.controller_id.into(),
                            value: v.value.into(),
                        },
                    },
                ));
            }
        }
    }
    buffer.sort_by_key(|(at, _)| at.to_owned());
    let mut midi_events = vec![];
    let mut running_at: TransportTime = 0;
    for (at, kind) in buffer {
        midi_events.push(TrackEvent {
            delta: (((at - running_at) as f64 / usec_per_tick as f64) as u32).into(),
            kind,
        });
        running_at = at;
    }
    midi_events
}

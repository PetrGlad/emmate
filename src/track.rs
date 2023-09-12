use midly::{MidiMessage, TrackEvent, TrackEventKind};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;
use crate::engine::TransportTime;

pub type Pitch = u8;
pub type ControllerId = u8;
pub type Level = u8;
pub type ChannelId = u8;

pub const MIDI_CC_MODWHEEL: ControllerId = 1;
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
    pub duration: Duration,
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
    /// Since the track beginning.
    pub at: Duration,
    pub event: LaneEventType,
}

impl LaneEvent {
    pub fn is_active(&self, at: Duration) -> bool {
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

#[derive(Debug, Default)]
pub struct Lane {
    // Notes should always be ordered by start time ascending. Not enforced yet.
    pub events: Vec<LaneEvent>,
}

pub fn to_lane_events(events: Vec<TrackEvent<'static>>, tick_duration: TransportTime) -> Vec<LaneEvent> {
    // TODO The offset calculations are very similar to ones in the engine. Can these be shared?
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

/// Reverse of to_lane_events
pub fn to_midi_events(events: &Vec<LaneEvent>, usec_per_tick: &u32) -> Vec<TrackEvent<'static>> {
    let mut midi_events: Vec<TrackEvent<'static>> = vec![];
    todo!();
    // for ev in events {
    //     at += ev.delta.as_int() as u64 * tick_duration;
    //     match ev.kind {
    //         TrackEventKind::Midi { message, .. } => match message {
    //             MidiMessage::NoteOn { key, .. } => {
    //                 ons.insert(key.as_int() as Pitch, (at, message));
    //             }
    //             MidiMessage::NoteOff { key, .. } => {
    //                 let on = ons.remove(&(key.as_int() as Pitch));
    //                 match on {
    //                     Some((t, MidiMessage::NoteOn { key, vel })) => {
    //                         lane_events.push(LaneEvent {
    //                             at: Duration::from_micros(t),
    //                             event: LaneEventType::Note(Note {
    //                                 duration: Duration::from_micros(at - t),
    //                                 pitch: key.as_int() as Pitch,
    //                                 velocity: vel.as_int() as Level,
    //                             }),
    //                         });
    //                     }
    //                     None => eprintln!("INFO NoteOff event without NoteOn {:?}", ev),
    //                     _ => panic!("ERROR Unexpected state: {:?} event in \"on\" queue.", on),
    //                 }
    //             }
    //             MidiMessage::Controller { controller, value } => lane_events.push(LaneEvent {
    //                 at: Duration::from_micros(at),
    //                 event: LaneEventType::Controller(ControllerSetValue {
    //                     controller_id: controller.into(),
    //                     value: value.into(),
    //                 }),
    //             }),
    //             _ => eprintln!("DEBUG Event ignored {:?}", ev),
    //         },
    //         _ => (),
    //     };
    // }
    // // Notes are collected after they complete, This mixes the ordering with immediate events.
    // lane_events.sort_by_key(|ev| ev.at.as_micros());
    // lane_events
}


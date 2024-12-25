use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;

use midly::num::u4;
use midly::{MidiMessage, TrackEventKind};
use serde::{Deserialize, Serialize};

use crate::changeset::{EventAction, EventActionsList, Snapshot};
use crate::common::Time;
use crate::midi;
use crate::range::{Range, RangeLike};
use crate::util::{is_ordered, IdSeq};

pub type Pitch = u8;
pub type ControllerId = u8;
pub type Level = u8;
pub type ChannelId = u8;
pub type EventId = u64;

pub const MAX_LEVEL: Level = 127; // Should be equal to u7::max_value().as_int();

#[allow(dead_code)]
pub const MIDI_CC_MODWHEEL_ID: ControllerId = 1;
// Damper pedal
pub const MIDI_CC_SUSTAIN_ID: ControllerId = 64;

pub fn is_cc_switch_on(x: Level) -> bool {
    // Not using crappy SLP3-D anyway.
    x >= 64
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub struct Note {
    pub pitch: Pitch,
    pub velocity: Level,
    pub duration: Time,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub struct ControllerSetValue {
    pub controller_id: ControllerId,
    pub value: Level,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub enum TrackEventType {
    Note(Note),
    Controller(ControllerSetValue),
    Bookmark,
}

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct TrackEvent {
    pub id: EventId,
    pub at: Time, // Since the track's beginning.
    pub event: TrackEventType,
}

impl TrackEvent {
    pub fn is_active_at(&self, at: Time) -> bool {
        match &self.event {
            TrackEventType::Note(n) => (self.at, self.at + n.duration).contains(&at),
            _ => false,
        }
    }

    pub fn intersects(&self, time_range: &Range<Time>) -> bool {
        match &self.event {
            TrackEventType::Note(n) => time_range.intersects(&(self.at, self.at + n.duration)),
            TrackEventType::Bookmark | TrackEventType::Controller(_) => {
                time_range.contains(&self.at)
            }
        }
    }
}

impl PartialOrd for TrackEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(&other))
    }
}

impl Ord for TrackEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        /*
        Ordering events by track time, while ensuring sorting always produces the same
        sequence every time. The time ordering is important for playback and editing while unique
        sort order ensures we do not have any surprises when changing or serializing the track.
        */
        (self.at, &self.event, self.id).cmp(&(other.at, &other.event, other.id))
    }
}

#[derive(Debug, Default, Clone)]
pub struct Track {
    /* Events must always be kept ordered by start time ascending.
    This is a requirement of TrackSource. */
    pub events: Vec<TrackEvent>,
}

impl Track {
    pub fn reset(&mut self, snapshot: Snapshot) {
        self.events = snapshot.events;
    }

    fn index_events(&self) -> HashMap<EventId, TrackEvent> {
        let mut track_map = HashMap::with_capacity(self.events.len());
        for ev in &self.events {
            track_map.insert(ev.id, ev.clone());
        }
        track_map
    }

    fn splat_events(&mut self, indexed: &HashMap<EventId, TrackEvent>) {
        self.events = indexed.values().cloned().collect();
        self.events.sort();
    }

    pub fn patch(&mut self, changes: &EventActionsList) {
        let mut track_map = self.index_events();
        for ea in changes {
            match ea.after() {
                Some(ev) => {
                    assert_eq!(
                        track_map.insert(ev.id, ev.clone()).is_some(),
                        matches!(ea, EventAction::Update(_, _))
                    );
                }
                None => {
                    assert!(track_map.remove(&ea.event_id()).is_some());
                }
            }
        }
        self.splat_events(&track_map);
    }

    pub fn commit(&mut self) {
        assert!(is_ordered(&self.events));
    }

    pub fn insert_event(&mut self, ev: TrackEvent) {
        let idx = self.events.partition_point(|x| x < &ev);
        self.events.insert(idx, ev);
        self.commit();
    }

    pub fn max_time(&self) -> Time {
        // Looks cumbersome. Maybe this is a case for handling MIDI (-like) events directly (see README).
        let mut result = 0;
        for ev in &self.events {
            let end_time = match &ev.event {
                TrackEventType::Note(Note { duration, .. }) => ev.at + duration,
                TrackEventType::Controller(_) => ev.at,
                TrackEventType::Bookmark => ev.at,
            };
            result = Time::max(result, end_time);
        }
        result
    }
}

pub fn from_midi_events(
    id_seq: &IdSeq,
    events: Vec<midly::TrackEvent<'static>>,
    tick_duration: Time,
) -> Vec<TrackEvent> {
    // TODO The offset calculations are very similar to ones in the engine. Can these be shared?
    let mut ons: HashMap<Pitch, (Time, MidiMessage)> = HashMap::new();
    let mut track_events = vec![];
    let mut at: Time = 0;
    for ev in events {
        at += ev.delta.as_int() as Time * tick_duration;
        match ev.kind {
            TrackEventKind::Midi { message, .. } => match message {
                MidiMessage::NoteOn { key, .. } => {
                    ons.insert(key.as_int() as Pitch, (at, message));
                }
                MidiMessage::NoteOff { key, .. } => {
                    let on = ons.remove(&(key.as_int() as Pitch));
                    match on {
                        Some((t, MidiMessage::NoteOn { key, vel })) => {
                            track_events.push(TrackEvent {
                                id: id_seq.next(),
                                at: t,
                                event: TrackEventType::Note(Note {
                                    duration: at - t,
                                    pitch: key.as_int() as Pitch,
                                    velocity: vel.as_int() as Level,
                                }),
                            });
                        }
                        None => log::warn!("NoteOff event without NoteOn {:?}", ev),
                        _ => panic!("ERROR Unexpected state: {:?} event in \"on\" queue.", on),
                    }
                }
                MidiMessage::Controller { controller, value } => track_events.push(TrackEvent {
                    id: id_seq.next(),
                    at,
                    event: TrackEventType::Controller(ControllerSetValue {
                        controller_id: controller.into(),
                        value: value.into(),
                    }),
                }),
                _ => log::trace!("Event ignored {:?}", ev),
            },
            _ => (),
        };
    }
    // Notes are collected after they complete, This mixes the ordering with immediate events.
    track_events.sort_by_key(|ev| ev.at);
    track_events
}

pub fn import_smf(id_seq: &IdSeq, file_path: &PathBuf) -> Vec<TrackEvent> {
    let data = std::fs::read(&file_path).unwrap();
    let events = midi::load_smf(&data);
    from_midi_events(&id_seq, events.0, events.1 as Time)
}

pub fn export_smf(events: &Vec<TrackEvent>, file_path: &PathBuf) {
    let usec_per_tick = 26u32;
    let midi_events = to_midi_events(&events, usec_per_tick);
    let mut binary = Vec::new();
    midi::serialize_smf(midi_events, usec_per_tick, &mut binary).expect("Cannot store SMF track.");
    std::fs::write(&file_path, binary).expect(&*format!("Cannot save to {}", &file_path.display()));
}

/// Reverse of from_midi_events
pub fn to_midi_events(
    events: &Vec<TrackEvent>,
    usec_per_tick: u32,
) -> Vec<midly::TrackEvent<'static>> {
    let channel = u4::from(0); // Channel hard coded.
    let mut buffer: Vec<(Time, TrackEventKind)> = vec![];
    for ev in events {
        match &ev.event {
            TrackEventType::Note(n) => {
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
            TrackEventType::Controller(v) => {
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
            TrackEventType::Bookmark => (), // Not a MIDI event.
        }
    }
    buffer.sort_by_key(|(at, _)| at.to_owned());
    let mut midi_events = vec![];
    let mut running_at: Time = 0;
    for (at, kind) in buffer {
        midi_events.push(midly::TrackEvent {
            delta: (((at - running_at) as f64 / usec_per_tick as f64) as u32).into(),
            kind,
        });
        running_at = at;
    }
    midi_events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_load() {
        let id_seq = IdSeq::new(0);
        let path_short = PathBuf::from("./test/files/short.mid");
        let events = import_smf(&id_seq, &path_short);
        assert_eq!(events.len(), 10);
        let path_exported = PathBuf::from("./target/test_track_load.mid");
        export_smf(&events, &path_exported);

        // The recorded SMD may have some additional system/heartbeat events,
        // so comparing the sequence only after a save.
        let id_seq = IdSeq::new(0);
        let events2 = import_smf(&id_seq, &path_exported);
        assert_eq!(events2.len(), 10);
        assert_eq!(events, events2);
    }
}

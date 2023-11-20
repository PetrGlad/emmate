use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::path::PathBuf;

use midly::num::u4;
use midly::{MidiMessage, TrackEventKind};
use serde::{Deserialize, Serialize};

use crate::changeset::{Changeset, EventAction, EventFn};
use crate::common::Time;
use crate::midi;
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
    // Pianoteq seem to support continuous damper values, may support this later.
    // Not using crappy SLP3-D anyway.
    x >= 64
}

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct Note {
    pub pitch: Pitch,
    pub velocity: Level,
    pub duration: Time,
}

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct ControllerSetValue {
    pub controller_id: ControllerId,
    pub value: Level,
}

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub enum TrackEventType {
    Note(Note),
    Controller(ControllerSetValue),
}

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct TrackEvent {
    pub id: EventId,
    pub at: Time, // Since the track beginning.
    pub event: TrackEventType,
}

impl TrackEvent {
    pub fn is_active(&self, at: Time) -> bool {
        match &self.event {
            TrackEventType::Note(n) => (self.at..(self.at + n.duration)).contains(&at),
            _ => false,
        }
    }
}

impl PartialOrd for TrackEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // TODO Maybe consider complete comparison (including actual events)
        //      to avoid ambiguities in sorting.
        Some(self.at.cmp(&other.at))
    }
}

impl Ord for TrackEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(&other).unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TimeSelection {
    pub from: Time,
    pub to: Time,
}

impl TimeSelection {
    pub fn length(&self) -> Time {
        self.to - self.from
    }

    pub fn contains(&self, at: Time) -> bool {
        self.from <= at && at < self.to
    }

    pub fn before(&self, at: Time) -> bool {
        self.to <= at
    }

    pub fn after_start(&self, at: Time) -> bool {
        self.from <= at
    }

    pub fn is_empty(&self) -> bool {
        self.to - self.from <= 0
    }
}

#[derive(Debug, Default)]
pub struct Track {
    /* Events should always be kept ordered by start time ascending.
    This is a requirement of TrackSource. */
    pub events: Vec<TrackEvent>,
    pub id_seq: IdSeq,
}

impl Track {
    pub fn import_smf(&mut self, file_path: &PathBuf) {
        let data = std::fs::read(&file_path).unwrap();
        let events = midi::load_smf(&data);
        self.events = from_midi_events(&mut self.id_seq, events.0, events.1 as Time);
        self.commit();
    }

    pub fn export_smf(&self, file_path: &PathBuf) {
        let usec_per_tick = 26u32;
        let midi_events = to_midi_events(&self.events, usec_per_tick);
        let mut binary = Vec::new();
        midi::serialize_smf(midi_events, usec_per_tick, &mut binary)
            .expect("Cannot store SMF track.");
        std::fs::write(&file_path, binary)
            .expect(&*format!("Cannot save to {}", &file_path.display()));
    }

    pub fn patch(&mut self, changeset: &Changeset) {
        let mut track_map = HashMap::with_capacity(self.events.len());
        for ev in &self.events {
            track_map.insert(ev.id, ev);
        }
        for ea in changeset.changes.values() {
            match ea.after() {
                Some(ev) => {
                    track_map.insert(ev.id, ev);
                }
                None => {
                    track_map.remove(&ea.event_id());
                }
            }
        }
        self.events = track_map.iter().map(|(_id, ev)| *ev).cloned().collect();
        self.events.sort();
    }

    pub fn commit(&mut self) {
        assert!(is_ordered(&self.events));
    }

    pub fn insert_event(&mut self, ev: TrackEvent) {
        let idx = self.events.partition_point(|x| x < &ev);
        self.events.insert(idx, ev);
        self.commit();
    }

    pub fn tape_cut(&mut self, time_selection: &TimeSelection, changeset: &mut Changeset) {
        dbg!("tape_cut", time_selection);
        self.events.retain(|ev| !time_selection.contains(ev.at));
        self.shift_events(
            &|ev| time_selection.before(ev.at),
            -(time_selection.length() as i64),
            changeset,
        );
        self.commit();
    }

    pub fn tape_insert(&mut self, time_selection: &TimeSelection, changeset: &mut Changeset) {
        dbg!("tape_insert", time_selection);
        self.shift_events(
            &|ev| time_selection.after_start(ev.at),
            time_selection.length() as i64,
            changeset,
        );
        self.commit();
    }

    pub fn shift_tail(&mut self, at: &Time, dt: i64) {
        dbg!("tail_shift", at, dt);
        let mut changeset = Changeset::empty();
        self.shift_events(&|ev| &ev.at > at, dt, &mut changeset);
        self.patch(&changeset);
        self.commit();
    }

    pub fn shift_events<Pred: Fn(&TrackEvent) -> bool>(
        &mut self,
        selector: &Pred,
        d: i64,
        changeset: &mut Changeset,
    ) {
        self.edit_events(
            selector,
            &move |ev| {
                let mut nev = ev.clone();
                nev.at += d;
                Some(EventAction::Update(ev.clone(), nev))
            },
            changeset,
        );

        //// // Should do this only for out-of-order events. Brute-forcing for now.
        //// self.events.sort();
        ///// self.commit();
    }

    pub fn edit_events<Selector: Fn(&TrackEvent) -> bool>(
        &mut self,
        selector: &Selector,
        action: &EventFn,
        changeset: &mut Changeset,
    ) {
        for ev in &mut self.events {
            if selector(&ev) {
                if let Some(a) = action(&ev) {
                    changeset.add(a);
                }
            }
        }
        // TODO Commit?
    }

    pub fn delete_events(&mut self, event_ids: &HashSet<EventId>, changeset: &mut Changeset) {
        self.events.retain(|ev| {
            if !event_ids.contains(&ev.id) {
                true
            } else {
                changeset.add(EventAction::Delete(ev.clone()));
                false
            }
        });
        self.commit();
    }

    pub fn max_time(&self) -> Time {
        // Looks cumbersome. Maybe this is a case for handling MIDI (-like) events directly (see README).
        let mut result = 0;
        for ev in &self.events {
            let end_time = match &ev.event {
                TrackEventType::Note(Note { duration, .. }) => ev.at + duration,
                TrackEventType::Controller(_) => ev.at,
            };
            result = Time::max(result, end_time);
        }
        result
    }
}

pub fn from_midi_events(
    id_seq: &mut IdSeq,
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
                        None => eprintln!("INFO NoteOff event without NoteOn {:?}", ev),
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
                _ => eprintln!("DEBUG Event ignored {:?}", ev),
            },
            _ => (),
        };
    }
    // Notes are collected after they complete, This mixes the ordering with immediate events.
    track_events.sort_by_key(|ev| ev.at);
    track_events
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
        let mut track = Track::default();
        assert!(track.events.is_empty());

        let path = PathBuf::from("./target/test_track_load.mid");
        track.save_to(&path);
        track.load_from(&path);
        assert!(track.events.is_empty());

        let short = PathBuf::from("./test/files/short.mid");
        track.load_from(&short);
        assert_eq!(track.events.len(), 10);
        track.save_to(&path);

        // The recorded SMD may have some additional system/heartbeat events,
        // so comparing the sequence only after a save.
        let mut track_loaded = Track::default();
        track_loaded.load_from(&path);
        assert_eq!(track_loaded.events.len(), 10);
        assert_eq!(track.events, track_loaded.events);
    }

    fn make_test_track() -> Track {
        let mut events: Vec<TrackEvent> = vec![];
        events.push(TrackEvent {
            id: 10,
            at: 10,
            event: TrackEventType::Controller(ControllerSetValue {
                controller_id: 13,
                value: 55,
            }),
        });
        events.push(TrackEvent {
            id: 20,
            at: 14,
            event: TrackEventType::Note(Note {
                pitch: 10,
                velocity: 20,
                duration: 30,
            }),
        });
        events.push(TrackEvent {
            id: 30,
            at: 15,
            event: TrackEventType::Controller(ControllerSetValue {
                controller_id: 44,
                value: 60,
            }),
        });
        events.push(TrackEvent {
            id: 40,
            at: 20,
            event: TrackEventType::Controller(ControllerSetValue {
                controller_id: 13,
                value: 66,
            }),
        });
        Track::new(events)
    }

    #[test]
    fn cc_value_at() {
        let track = make_test_track();

        assert_eq!(55, track.cc_value_at(&20, &13));
        assert_eq!(66, track.cc_value_at(&21, &13));
        assert_eq!(60, track.cc_value_at(&21, &44));
        assert_eq!(0, track.cc_value_at(&21, &99));
        assert_eq!(0, track.cc_value_at(&0, &99));
    }

    #[test]
    fn set_damper_to() {
        let mut track = make_test_track();
        track.set_damper_to((14, 17), true);

        let expected_ids: Vec<EventId> = vec![10, 0, 20, 30, 1, 40];
        assert_eq!(
            expected_ids,
            track
                .events
                .iter()
                .map(|ev| ev.id)
                .collect::<Vec<EventId>>()
        );

        let expected_states: Vec<Option<bool>> = vec![
            Some(false),
            Some(true),
            None,
            Some(false),
            Some(false),
            Some(true),
        ];
        assert_eq!(
            expected_states,
            track
                .events
                .iter()
                .map(|ev| if let TrackEventType::Controller(ctl) = &ev.event {
                    Some(is_cc_switch_on(ctl.value))
                } else {
                    None
                })
                .collect::<Vec<Option<bool>>>()
        );
    }
}

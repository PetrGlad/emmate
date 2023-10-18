use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::SeqCst;

use midly::num::{u4, u7};
use midly::{MidiMessage, TrackEvent, TrackEventKind};

use crate::common::VersionId;
use crate::engine::TransportTime;
use crate::util::{is_ordered, range_contains};
use crate::{midi, util};

pub type Pitch = u8;
pub type ControllerId = u8;
pub type Level = u8;
pub type ChannelId = u8;
pub type EventId = u64;

pub const MIDI_CC_MODWHEEL_ID: ControllerId = 1;
// Damper pedal
pub const MIDI_CC_SUSTAIN_ID: ControllerId = 64;

pub fn is_cc_switch_on(x: Level) -> bool {
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

    pub fn after_start(&self, at: TransportTime) -> bool {
        self.from <= at
    }
}

#[derive(Debug, Default)]
pub struct Lane {
    /* Events should always be kept ordered by start time ascending.
    This is a requirement of TrackSource. */
    pub events: Vec<LaneEvent>,
    pub version: VersionId,
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

    pub fn add_note(
        &mut self,
        time_range: (TransportTime, TransportTime),
        pitch: Pitch,
        level: Level,
    ) {
        let ev = LaneEvent {
            id: self.id_seq.fetch_add(1, SeqCst),
            at: time_range.0,
            event: LaneEventType::Note(Note {
                pitch,
                velocity: level,
                duration: time_range.1 - time_range.0,
            }),
        };
        self.insert_event(ev);
    }

    fn commit(&mut self) {
        assert!(is_ordered(&self.events));
        self.version += 1;
    }

    pub fn insert_event(&mut self, ev: LaneEvent) {
        let idx = self.events.partition_point(|x| x < &ev);
        self.events.insert(idx, ev);
        self.commit();
    }

    fn clear_cc_events(&mut self, time_range: util::Range<TransportTime>, cc_id: ControllerId) {
        let mut i = 0;
        loop {
            if let Some(ev) = self.events.get(i) {
                if range_contains(time_range, ev.at) {
                    if let LaneEventType::Controller(ev) = &ev.event {
                        if ev.controller_id == cc_id {
                            self.events.remove(i);
                            continue;
                        }
                    }
                }
                i += 1;
            } else {
                break;
            }
        }
    }

    pub fn set_damper_to(&mut self, time_range: util::Range<TransportTime>, on: bool) {
        dbg!("set_damper_range", time_range, on);
        let on_before = is_cc_switch_on(self.cc_value_at(&time_range.0, &MIDI_CC_SUSTAIN_ID));
        let on_after = is_cc_switch_on(self.cc_value_at(&time_range.1, &MIDI_CC_SUSTAIN_ID));

        self.clear_cc_events(time_range, MIDI_CC_SUSTAIN_ID);

        if on {
            if !on_before {
                let on_ev = self.sustain_event(&time_range.0, true);
                self.insert_event(on_ev);
            }
            if !on_after {
                let off_ev = self.sustain_event(&time_range.1, false);
                self.insert_event(off_ev);
            }
        } else {
            if on_before {
                let off_ev = self.sustain_event(&time_range.0, false);
                self.insert_event(off_ev);
            }
            if on_after {
                let on_ev = self.sustain_event(&time_range.1, true);
                self.insert_event(on_ev);
            }
        }
        self.commit();
    }

    fn cc_value_at(&self, at: &TransportTime, cc_id: &ControllerId) -> Level {
        let mut idx = self.events.partition_point(|x| x.at < *at);
        while idx > 0 {
            idx -= 1;
            if let Some(ev) = self.events.get(idx) {
                if let LaneEventType::Controller(cc) = &ev.event {
                    if cc.controller_id == *cc_id {
                        return cc.value;
                    }
                }
            }
        }
        return 0; // default
    }

    fn sustain_event(&mut self, at: &TransportTime, on: bool) -> LaneEvent {
        LaneEvent {
            id: next_id(&mut self.id_seq),
            at: *at,
            event: LaneEventType::Controller(ControllerSetValue {
                controller_id: MIDI_CC_SUSTAIN_ID,
                value: if on {
                    u7::max_value().as_int() as Level
                } else {
                    0
                },
            }),
        }
    }

    pub fn tape_cut(&mut self, time_selection: &TimeSelection) {
        dbg!("tape_cut", time_selection);
        self.events.retain(|ev| !time_selection.contains(ev.at));
        self.shift_events(
            &|ev| time_selection.before(ev.at),
            -(time_selection.length() as i64),
        );
        self.commit();
    }

    pub fn tape_insert(&mut self, time_selection: &TimeSelection) {
        dbg!("tape_insert", time_selection);
        self.shift_events(
            &|ev| time_selection.after_start(ev.at),
            time_selection.length() as i64,
        );
        self.commit();
    }

    pub fn shift_tail(&mut self, at: &TransportTime, dt: i64) {
        dbg!("tail_shift", at, dt);
        self.shift_events(&|ev| &ev.at > at, dt);
        self.commit();
    }

    pub fn shift_events<Pred: Fn(&LaneEvent) -> bool>(&mut self, selector: &Pred, d: i64) {
        for ev in &mut self.events {
            if selector(ev) {
                ev.at = ev
                    .at
                    .checked_add_signed(d)
                    // Need to show some visual feedback and just cancel the operation instead.
                    .expect("Should not shift event into negative times.");
            }
        }
        // Should do this only for out-of-order events. Brute-forcing for now.
        self.events.sort();
        self.commit();
    }

    // Is it worth it?
    pub fn edit_events<
        'a,
        T: 'a,
        Selector: Fn(&'a mut LaneEvent) -> Option<&'a mut T>,
        Action: Fn(&'a mut T),
    >(
        &'a mut self,
        selector: &Selector,
        action: &Action,
    ) {
        for ev in &mut self.events {
            if let Some(x) = selector(ev) {
                action(x);
            }
        }
    }

    pub fn delete_events(&mut self, event_ids: &HashSet<EventId>) {
        self.events.retain(|ev| !event_ids.contains(&ev.id));
        self.commit();
    }
}

fn next_id(id_seq: &mut AtomicU64) -> EventId {
    id_seq.fetch_add(1, SeqCst)
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
                                id: next_id(id_seq),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_load() {
        let mut lane = Lane::default();
        assert!(lane.events.is_empty());

        let path = PathBuf::from("./target/test_lane_load.mid");
        lane.save_to(&path);
        lane.load_from(&path);
        assert!(lane.events.is_empty());

        let short = PathBuf::from("./test/files/short.mid");
        lane.load_from(&short);
        assert_eq!(lane.events.len(), 10);
        lane.save_to(&path);

        // The recorded SMD may have some additional system/heartbeat events,
        // so comparing the sequence only after a save.
        let mut lane_loaded = Lane::default();
        lane_loaded.load_from(&path);
        assert_eq!(lane_loaded.events.len(), 10);
        assert_eq!(lane.events, lane_loaded.events);
    }

    fn make_test_lane() -> Lane {
        let mut lane = Lane::default();
        lane.events.push(LaneEvent {
            id: 10,
            at: 10,
            event: LaneEventType::Controller(ControllerSetValue {
                controller_id: 13,
                value: 55,
            }),
        });
        lane.events.push(LaneEvent {
            id: 20,
            at: 14,
            event: LaneEventType::Note(Note {
                pitch: 10,
                velocity: 20,
                duration: 30,
            }),
        });
        lane.events.push(LaneEvent {
            id: 30,
            at: 15,
            event: LaneEventType::Controller(ControllerSetValue {
                controller_id: 44,
                value: 60,
            }),
        });
        lane.events.push(LaneEvent {
            id: 40,
            at: 20,
            event: LaneEventType::Controller(ControllerSetValue {
                controller_id: 13,
                value: 66,
            }),
        });
        lane
    }

    #[test]
    fn cc_value_at() {
        let mut lane = make_test_lane();

        assert_eq!(55, lane.cc_value_at(&20, &13));
        assert_eq!(66, lane.cc_value_at(&21, &13));
        assert_eq!(60, lane.cc_value_at(&21, &44));
        assert_eq!(0, lane.cc_value_at(&21, &99));
        assert_eq!(0, lane.cc_value_at(&0, &99));
    }

    #[test]
    fn set_damper_to() {
        let mut lane = make_test_lane();
        lane.set_damper_to((14, 17), true);

        let expected_ids: Vec<EventId> = vec![10, 0, 20, 30, 1, 40];
        assert_eq!(
            expected_ids,
            lane.events.iter().map(|ev| ev.id).collect::<Vec<EventId>>()
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
            lane.events
                .iter()
                .map(|ev| if let LaneEventType::Controller(ctl) = &ev.event {
                    Some(is_cc_switch_on(ctl.value))
                } else {
                    None
                })
                .collect::<Vec<Option<bool>>>()
        );
    }
}

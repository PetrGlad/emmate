use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;

use midly::num::u4;
use midly::{MidiMessage, TrackEventKind};
use serde::{Deserialize, Serialize};

use crate::changeset::{EventAction, EventActionsList, Snapshot};
use crate::common::Time;
use crate::ev::{ControllerId, Level, Pitch, Velocity};
use crate::util::{is_ordered, is_ordered_by_key, IdSeq};
use crate::{ev, midi};

// Should be equal to u7::max_value().as_int();
// ".max_value" is not declared as const yet.
pub const MAX_LEVEL: Level = 127;

#[allow(dead_code)]
pub const MIDI_CC_MODWHEEL_ID: ControllerId = 1;
// Damper pedal
pub const MIDI_CC_SUSTAIN_ID: ControllerId = 64;

pub fn is_cc_switch_on(x: Level) -> bool {
    // Not using crappy SLP3-D anyway.
    x >= 64
}

/// Stave-history-unique event id.
pub type EventId = u64;

#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct Item<Ev> {
    pub id: EventId,
    pub at: Time, // Since the track's beginning.
    pub event: Ev,
}

// impl<Ev> Item<Ev> {
// }

// impl PartialOrd for ev::Item<Ev> {
//     fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
//         // TODO Maybe consider complete comparison (including actual events)
//         //      to avoid ambiguities in sorting.
//         Some(self.at.cmp(&other.at))
//     }
// }

// impl Ord for ev::Item {
//     fn cmp(&self, other: &Self) -> Ordering {
//         self.partial_cmp(&other).unwrap()
//     }
// }

/*
 TODO (refactoring, a big one, ???) Use MIDI-like events directly in the track.
  Currently notes have duration and correspond to 2 MIDI events ("on" and "off").
  New implementation would:
    0. Keep events separated by lanes (one for each pitch, CC control, or UI marker type).
       Events in each lane should  be ordered by time ascending.
    1. Instead of notes hold MIDI events of structs that directly correspond to a MIDI events.
    2. Treat track-time or note related markers (like bookmarks) as another event type.
    3. (optimally) Keep unsupported types of MIDI on the track also.
    4. (???) How selection and diff patches will refer to events then? Should we still have
       project-unique event ids? Or event id should be "lane_id:event_id"?
    5. starting and ending event of a note will have to be stored separately in the edit history.
    6. Map events to some internal convenient structs or provide some view functions directly
       into SMD events? Dealing with midir's 7 bit integers is cumbersome, I'd rather avoid that.
       MIDI v2 introduces more levels (1024?), which have to be also supported.
    7. Note change animations will have to be inferred. If an end of a note is affected
       then the range should be animated.
  Expected result:
    1. Export/import and playback would be more complex. In particular export procedure and
       track source for playback engine will have to scan all (non UI) lanes to see which event
       comes next. Import/load procedure will have to take single MIDI stream and group events by lane.
       On the other hand mapping to notes and back will be unnecessary.
    2. Have ability to keep unsupported events. More fidelity to input MIDI file in the exported data.
       Although this looks odd but it is possible to have two "on" events in a row for the same note.
       Not sure what "off" velocity affects, but adjusting it will also be possible.
       The new implementation may have a dedicated lane for events that are unsupported or ignored.
    3. CC events can be used as is without handling them as special case.
    4. Zoomed display optimization: only visible events can be selected for painting.
    5. Simplify some time operations at expense of note selection which will be trickier.
       For example playback resuming may need to look up previous or next note. At the moment
       this may require to scan whole track to the beginning or to the end.
*/

#[derive(Debug, Default, Clone)]
pub struct Track {
    pub items: Vec<ev::Item>,
}

impl Track {
    pub fn reset(&mut self, snapshot: Snapshot) {
        self.items = snapshot.events;
    }

    /// See also splat_events
    fn index_events(&self) -> HashMap<EventId, ev::Item> {
        let mut track_map = HashMap::with_capacity(self.items.len());
        for ev in &self.items {
            track_map.insert(ev.id, ev.clone());
        }
        track_map
    }

    /// Reverse of index_events
    fn splat_events(&mut self, indexed: &HashMap<EventId, ev::Item>) {
        self.items = indexed.values().cloned().collect();
        self.items.sort();
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
        assert!(is_ordered(&self.items));
    }

    pub fn insert_event(&mut self, ev: ev::Item) {
        let idx = self.items.partition_point(|x| x < &ev);
        self.items.insert(idx, ev);
        self.commit();
    }

    pub fn max_time(&self) -> Option<Time> {
        self.items.iter().map(|item| item.at).max()
    }
}

pub fn from_midi_events(
    id_seq: &IdSeq,
    events: Vec<midly::TrackEvent<'static>>,
    tick_duration: Time,
) -> Vec<ev::Item> {
    let mut items = vec![];
    let mut at: Time = 0;
    for ev in events {
        at += ev.delta.as_int() as Time * tick_duration;
        match ev.kind {
            TrackEventKind::Midi { message, .. } => match message {
                MidiMessage::NoteOn { key, vel } => {
                    items.push(ev::Item {
                        id: id_seq.next(),
                        at,
                        ev: ev::Type::Note(ev::Tone {
                            on: true,
                            pitch: key.as_int() as Pitch,
                            velocity: vel.as_int() as Velocity,
                        }),
                    });
                }
                MidiMessage::NoteOff { key, vel } => {
                    items.push(ev::Item {
                        id: id_seq.next(),
                        at,
                        ev: ev::Type::Note(ev::Tone {
                            on: false,
                            pitch: key.as_int() as Pitch,
                            velocity: vel.as_int() as Velocity,
                        }),
                    });
                }
                MidiMessage::Controller { controller, value } => items.push(ev::Item {
                    id: id_seq.next(),
                    at,
                    ev: ev::Type::Cc(ev::Cc {
                        controller_id: controller.into(),
                        value: value.into(),
                    }),
                }),
                _ => eprintln!("DEBUG Event ignored {:?}", ev),
            },
            _ => (),
        };
    }
    items
}

pub fn import_smf(id_seq: &IdSeq, file_path: &PathBuf) -> Vec<ev::Item> {
    let data = std::fs::read(&file_path).unwrap();
    let events = midi::load_smf(&data);
    from_midi_events(id_seq, events.0, events.1 as Time)
}

pub fn export_smf(events: &Vec<ev::Item>, file_path: &PathBuf) {
    let usec_per_tick = 26u32;
    let midi_events = to_midi_events(&events, usec_per_tick);
    let mut binary = Vec::new();
    midi::serialize_smf(midi_events, usec_per_tick, &mut binary).expect("Cannot store SMF track.");
    std::fs::write(&file_path, binary).expect(&*format!("Cannot save to {}", &file_path.display()));
}

/// Reverse of from_midi_events
pub fn to_midi_events(
    events: &Vec<ev::Item>,
    usec_per_tick: u32,
) -> Vec<midly::TrackEvent<'static>> {
    let channel = u4::from(0); // Channel hard coded.
    let mut buffer: Vec<(Time, TrackEventKind)> = vec![];
    for ev in events {
        match &ev.ev {
            ev::Type::Note(note) => {
                buffer.push((
                    ev.at,
                    TrackEventKind::Midi {
                        channel,
                        message: MidiMessage::NoteOn {
                            key: note.pitch.into(),
                            vel: note.velocity.into(),
                        },
                    },
                ));
            }
            ev::Type::Cc(cc) => {
                buffer.push((
                    ev.at,
                    TrackEventKind::Midi {
                        channel,
                        message: MidiMessage::Controller {
                            controller: cc.controller_id.into(),
                            value: cc.value.into(),
                        },
                    },
                ));
            }
            ev::Type::Bookmark(_) => (), // Not a MIDI event.
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

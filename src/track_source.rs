use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use sync_cow::SyncCow;

use crate::common::Time;
use crate::engine;
use crate::engine::{EngineEvent, EventSource};
use crate::midi::{controller_set, note_off, note_on};
use crate::track::Track;

pub struct TrackSource {
    /* Events must always be kept ordered by start
    time ascending (see seel() and next() methods). */
    track: Arc<SyncCow<Track>>,
    current_idx: usize,
    running_at: Time,
}

impl Debug for TrackSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "TrackSource{{i={}, t={}, len={}}}",
            self.current_idx,
            self.running_at,
            self.track.read().events.len()
        ))?;
        Ok(())
    }
}

impl TrackSource {
    pub fn new(track: Arc<SyncCow<Track>>) -> TrackSource {
        TrackSource {
            track,
            current_idx: 0,
            running_at: 0,
        }
    }
}

impl EventSource for TrackSource {
    fn is_running(&self) -> bool {
        true
    }

    fn seek(&mut self, at: &Time) {
        let track = self.track.read();
        let note_on_time = |i: usize| track.items.get(i).map(|ev| ev.at);
        // Seek back until we cross the `at`, then forward, to stop on the earliest event after
        // the `at` moment. That is, looking for sup of {ev | ev.t <= at}. Should work if the
        // target is both before and after the current one.
        // Note that the track may be modified since we last read it.
        while let Some(t) = note_on_time(self.current_idx) {
            if *at >= t {
                break;
            }
            if let Some(idx) = self.current_idx.checked_sub(1) {
                self.current_idx = idx;
            } else {
                break;
            }
        }
        if None == note_on_time(self.current_idx) {
            self.current_idx = 0;
        }
        while let Some(t) = note_on_time(self.current_idx) {
            if *at <= t {
                break;
            }
            self.current_idx += 1;
            if track.items.len() <= self.current_idx {
                break;
            }
        }
        self.running_at = *at;
    }

    fn next(&mut self, at: &Time) -> Vec<EngineEvent> {
        let track = self.track.read();
        let mut events = vec![];
        while self.current_idx < track.items.len() {
            let notes = &track.items;
            let event = &notes[self.current_idx];
            let running_at = event.at;
            if running_at > *at {
                return events;
            }
            self.running_at = running_at;
            match &event.event {
                ev::Type::Note(note) => {
                    events.push(EngineEvent {
                        at: running_at,
                        event: note_on(engine::MIDI_CHANNEL, note.pitch, note.velocity),
                    });
                    events.push(EngineEvent {
                        at: running_at + note.duration,
                        event: note_off(engine::MIDI_CHANNEL, note.pitch, note.velocity),
                    });
                }
                ev::Type::Controller(set_val) => {
                    events.push(EngineEvent {
                        at: running_at,
                        event: controller_set(
                            engine::MIDI_CHANNEL,
                            set_val.controller_id,
                            set_val.value,
                        ),
                    });
                }
                ev::Type::Bookmark => (), // Not an audible event.
            }
            self.current_idx += 1;
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use crate::track;
    use crate::track::ev::Item;

    use super::*;

    #[test]
    fn empty_track() {
        let track = Arc::new(SyncCow::new(Track::default()));
        let mut source = TrackSource::new(track);
        source.seek(&100_000i64);
        assert_eq!(source.running_at, 100_000);
        source.seek(&0);
        assert_eq!(source.running_at, 0);
        assert_eq!(source.current_idx, 0)
    }

    #[test]
    fn one_note() {
        let mut track = Track::default();
        track.items.push(ev::Item {
            id: 13,
            at: 1000,
            event: ev::Type::Note(track::Note {
                pitch: 55,
                velocity: 55,
                duration: 12,
            }),
        });
        let track = Arc::new(SyncCow::new(track));

        let mut source = TrackSource::new(track);
        source.seek(&0);
        assert_eq!(source.running_at, 0);
        assert_eq!(source.current_idx, 0);
        source.seek(&100i64);
        assert_eq!(source.running_at, 100);
        assert_eq!(source.current_idx, 0);
        source.seek(&2000i64);
        assert_eq!(source.running_at, 2000);
        assert_eq!(source.current_idx, 1)
    }
}

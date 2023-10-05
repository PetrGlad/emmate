use crate::engine::{EngineEvent, EventSource, TransportTime};
use crate::midi::{controller_set, note_off, note_on};
use crate::track::{Lane, LaneEventType};
use std::collections::BinaryHeap;
use std::sync::{Arc, RwLock};

pub struct TrackSource {
    track: Arc<RwLock<Lane>>,
    current_idx: usize,
    running_at: TransportTime,
}

impl TrackSource {
    pub fn new(track: Arc<RwLock<Lane>>) -> TrackSource {
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

    fn seek(&mut self, at: &TransportTime) {
        let track = self.track.read().expect("Cannot read track.");
        let note_on_time = |i: usize| track.events.get(i).map(|ev| ev.at);
        // Seek back until we cross the `at`, then forward, to stop on the earliest event after
        // the `at` moment. Should work if the target is both before and after the current one.
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
            if track.events.len() <= self.current_idx {
                break;
            }
        }
        self.running_at = *at;
    }

    fn next(&mut self, at: &TransportTime, queue: &mut BinaryHeap<EngineEvent>) {
        let track = self.track.read().expect("Cannot read track.");
        while self.current_idx < track.events.len() {
            let notes = &track.events;
            let event = &notes[self.current_idx];
            let running_at = event.at;
            if running_at > *at {
                return;
            }
            self.running_at = running_at;
            match &event.event {
                LaneEventType::Note(note) => {
                    queue.push(EngineEvent {
                        at: running_at,
                        event: note_on(1, note.pitch, note.velocity),
                    });
                    queue.push(EngineEvent {
                        at: running_at + note.duration,
                        event: note_off(1, note.pitch, note.velocity),
                    });
                }
                LaneEventType::Controller(set_val) => {
                    queue.push(EngineEvent {
                        at: running_at as u64,
                        event: controller_set(1, set_val.controller_id, set_val.value),
                    });
                }
            }
            self.current_idx += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track;
    use crate::track::LaneEvent;

    #[test]
    fn empty_lane() {
        let lane = Arc::new(RwLock::new(Lane::default()));
        let mut source = TrackSource::new(lane);
        source.seek(&100_000u64);
        assert_eq!(source.running_at, 100_000);
        source.seek(&0);
        assert_eq!(source.running_at, 0);
        assert_eq!(source.current_idx, 0)
    }

    #[test]
    fn one_note() {
        let mut lane = Lane::default();
        lane.events.push(LaneEvent {
            at: 1000,
            event: LaneEventType::Note(track::Note {
                pitch: 55,
                velocity: 55,
                duration: 12,
            }),
        });
        let track = Arc::new(RwLock::new(lane));

        let mut source = TrackSource::new(track);
        source.seek(&0);
        assert_eq!(source.running_at, 0);
        assert_eq!(source.current_idx, 0);
        source.seek(&100u64);
        assert_eq!(source.running_at, 100);
        assert_eq!(source.current_idx, 0);
        source.seek(&2000u64);
        assert_eq!(source.running_at, 2000);
        assert_eq!(source.current_idx, 1)
    }
}

use crate::engine::{EngineEvent, EventSource, TransportTime};
use crate::midi::{controller_set, note_off, note_on};
use crate::track::{Lane, LaneEventType};
use std::collections::BinaryHeap;
use std::sync::Arc;

pub struct TrackSource {
    track: Arc<Box<Lane>>,
    current_idx: usize,
    running_at: TransportTime,
}

impl TrackSource {
    pub fn new(track: Arc<Box<Lane>>) -> TrackSource {
        TrackSource {
            track,
            current_idx: 0,
            running_at: 0,
        }
    }

    fn note_on_time(&self, i: usize) -> Option<u64> {
        self.track.events.get(i).map(|ev| ev.at.as_micros() as u64)
    }
}

impl EventSource for TrackSource {
    fn is_running(&self) -> bool {
        true
    }

    fn seek(&mut self, at: &TransportTime) {
        // Seek back until we cross the `at`, then forward, to stop on the earliest event after
        // the `at` moment. Should work if the target is both before and after the current one.
        while let Some(t) = self.note_on_time(self.current_idx) {
            if *at >= t {
                break;
            }
            self.current_idx -= 1;
            if self.current_idx <= 0 {
                break;
            }
        }
        if None == self.note_on_time(self.current_idx) {
            self.current_idx = 0;
        }
        while let Some(t) = self.note_on_time(self.current_idx) {
            if *at <= t {
                break;
            }
            self.current_idx += 1;
            if self.track.events.len() <= self.current_idx {
                break;
            }
        }
        self.running_at = *at;
    }

    fn next(&mut self, at: &TransportTime, queue: &mut BinaryHeap<EngineEvent>) {
        while self.current_idx < self.track.events.len() {
            let notes = &self.track.events;
            let event = &notes[self.current_idx];
            let running_at = event.at.as_micros() as u64;
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
                        at: running_at + note.duration.as_micros() as u64,
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

    #[test]
    fn empty_lane() {
        let lane = Arc::new(Box::new(Lane { events: vec![] }));
        let mut source = TrackSource::new(lane);
        source.seek(&100_000u64);
        assert_eq!(source.running_at, 100_000);
        source.seek(&0);
        assert_eq!(source.running_at, 0);
    }
}

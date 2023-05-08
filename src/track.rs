use crate::engine::{EngineEvent, EventSource, TransportTime};
use crate::midi::{note_off, note_on};
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::time::Duration;

pub type Pitch = u8;
pub type Velocity = u8;
pub type ChannelId = u8;
pub type TrackTime = Duration;

#[derive(Debug, Eq, PartialEq)]
pub struct Note {
    pub pitch: Pitch,
    pub velocity: Velocity,
    // Since the track beginning.
    pub on: Duration,
    pub duration: Duration,
}

#[derive(Debug, Default)]
pub struct Track {
    /**
       Notes should always be ordered by start time ascending.
    */
    pub notes: Vec<Note>,
}

pub struct TrackSource {
    track: Arc<Box<Track>>,
    current_idx: usize,
    running_at: TransportTime,
}

impl TrackSource {
    pub fn new(track: Arc<Box<Track>>) -> TrackSource {
        TrackSource {
            track,
            current_idx: 0,
            running_at: 0,
        }
    }
}

impl EventSource for TrackSource {
    fn is_running(&self) -> bool {
        self.current_idx < self.track.notes.len()
    }

    fn reset(&mut self, at: &TransportTime) {
        assert!(
            self.running_at > *at,
            "Back reset is not supported yet for SmfSource."
        );
        // TODO Seek index to a suitable previous position (can be behind the current one also).
        self.running_at = at.to_owned();
    }

    fn next(&mut self, at: &TransportTime, queue: &mut BinaryHeap<EngineEvent>) {
        while self.is_running() {
            let notes = &self.track.notes;
            let note = &notes[self.current_idx];
            let running_at = note.on.as_micros() as u64;
            if running_at > *at {
                return;
            }
            self.running_at = running_at;
            queue.push(EngineEvent {
                at: running_at,
                event: note_on(1, note.pitch, note.velocity),
            });
            queue.push(EngineEvent {
                at: running_at + note.duration.as_micros() as u64,
                event: note_off(1, note.pitch, note.velocity),
            });
            self.current_idx += 1;
        }
    }
}

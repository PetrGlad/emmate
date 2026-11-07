use std::time::{Duration, Instant};

use crate::engine::EngineHandle;
use crate::track::Pitch;
use crate::{engine, midi};

const SAMPLE_DURATION: Duration = Duration::from_millis(500);

/// Plays a note sample, ensuring it is not spammed too often.
/// The note-off is scheduled by the engine, so it works while playback is paused.
#[derive(Debug)]
pub struct NoteTester {
    engine_command_send: EngineHandle,
    busy_until: Instant,
}

impl NoteTester {
    pub fn new(engine_command_send: EngineHandle) -> Self {
        NoteTester {
            engine_command_send,
            busy_until: Instant::now(),
        }
    }

    pub fn play_sample(&mut self, pitch: &Pitch) {
        let now = Instant::now();
        if now < self.busy_until {
            return; // Just try next time.
        }
        self.busy_until = now + SAMPLE_DURATION;
        let pitch = *pitch;
        let _ = self.engine_command_send.send(Box::new(move |engine| {
            engine.process(midi::note_on(engine::MIDI_CHANNEL, pitch, 64));
            engine.play_after(
                SAMPLE_DURATION,
                midi::note_off(engine::MIDI_CHANNEL, pitch, 64),
            );
        }));
    }
}

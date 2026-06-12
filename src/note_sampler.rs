use crate::engine::EngineCommand;
use crate::track::{ChannelId, Level, Pitch};
use crate::{engine, midi};
use midly::MidiMessage;
use midly::live::LiveEvent;
use midly::num::{u4, u7};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

// Plays a note sample, ensuring it is not spammed too often.
#[derive(Debug)]
pub struct NoteTester {
    engine_command_send: mpsc::Sender<Box<EngineCommand>>,
    lock: Arc<Mutex<()>>,
}

impl NoteTester {
    pub fn new(engine_command_send: mpsc::Sender<Box<EngineCommand>>) -> Self {
        NoteTester {
            engine_command_send,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn play_sample(&mut self, pitch: &Pitch) {
        let command_sender = self.engine_command_send.clone();
        // I would like to re-use engine's scheduling here (send a note with duration
        // for playback), but playback depends on play/pause state, meaning engine will not play
        // anything when paused. Alternatively this could have been implemented with async.
        // Having this implementation util there's a better idea.
        let pitch = *pitch;
        let lock = self.lock.clone();
        thread::spawn(move || {
            let Ok(lock) = lock.try_lock() else {
                return; // Just try next time.
            };
            command_sender
                .send(Box::new(move |engine| {
                    engine.process(midi::note_on(engine::MIDI_CHANNEL, pitch, 64))
                }))
                .unwrap();
            sleep(Duration::from_millis(500));
            command_sender
                .send(Box::new(move |engine| {
                    engine.process(midi::note_off(engine::MIDI_CHANNEL, pitch, 64))
                }))
                .unwrap();
        });
    }
}

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use midir::MidiOutputConnection;
use midly::live::LiveEvent;
use midly::num::u7;
use midly::MidiMessage;
use midly::MidiMessage::NoteOff;

use crate::common::Time;
use crate::ev::ChannelId;
use crate::track::MIDI_CC_SUSTAIN_ID;

pub const MIDI_CHANNEL: ChannelId = 1;

/** Event that is produced by engine. */
#[derive(Clone, Debug)]
pub enum StatusEvent {
    Time(Time),
}

/// A sound event to be rendered by the engine at given time.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct EngineEvent {
    pub at: Time,
    pub event: LiveEvent<'static>,
}

pub type StatusEventReceiver = dyn Fn(StatusEvent) -> () + Send;

impl Ord for EngineEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Ideally the event should also be compared to make the comparison unambiguous.
        // This should not matter for scheduling though.
        other.at.cmp(&self.at)
    }
}

impl PartialOrd for EngineEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub trait EventSource {
    /** Return false when no events will be produced from the source anymore.
    Use this to detach it from the engine. */
    fn is_running(&self) -> bool;
    /** Reset the current source's time to this moment. */
    fn seek(&mut self, at: &Time);
    /** The next event to be played at the instant. On subsequent
    calls instants must not decrease unless a reset call sets back the time. */
    fn next(&mut self, at: &Time) -> Vec<EngineEvent>;
}

type EventSourceHandle = dyn EventSource + Send;

pub type EngineCommand = dyn FnOnce(&mut Engine) + Send;

pub struct Engine {
    midi_output: MidiOutputConnection,
    sources: Vec<Box<EventSourceHandle>>,
    running_at: Time,
    reset_at: Instant,
    paused: bool,
    status_receiver: Option<Box<StatusEventReceiver>>,
    command_receiver: mpsc::Receiver<Box<EngineCommand>>,
    command_sender: mpsc::Sender<Box<EngineCommand>>,
    current_sustain: Option<LiveEvent<'static>>,
    queue: BinaryHeap<EngineEvent>,
}

impl Engine {
    pub fn new(
        midi_output: MidiOutputConnection,
        command_sender: mpsc::Sender<Box<EngineCommand>>,
        command_receiver: mpsc::Receiver<Box<EngineCommand>>,
    ) -> Engine {
        Engine {
            midi_output,
            sources: Vec::new(),
            running_at: 0,
            reset_at: Instant::now(),
            paused: false,
            status_receiver: None,
            current_sustain: None,
            command_receiver,
            command_sender,
            queue: BinaryHeap::new(),
        }
    }

    pub fn start(self) -> Arc<Mutex<Engine>> {
        let engine = Arc::new(Mutex::new(self));
        let engine2 = engine.clone();
        thread::spawn(move || {
            engine2.lock().unwrap().seek(0);
            loop {
                thread::sleep(Duration::from_micros(3_000)); // TODO (improvement) Use async instead
                let lock = engine2.lock();
                if let Err(_) = lock {
                    continue; // Will try next time.
                }
                let mut locked = lock.unwrap();
                let pending_commands: Vec<Box<EngineCommand>> =
                    locked.command_receiver.try_iter().collect();
                for command in pending_commands {
                    command(&mut locked);
                }
                if locked.paused {
                    continue;
                };
                locked.sources.retain(|s| s.is_running());
                Self::update_track_time(&mut locked);
                let transport_time = locked.running_at;
                for ev in locked
                    .sources
                    .iter_mut()
                    .map(|s| s.next(&transport_time))
                    .flatten()
                    .collect::<Vec<EngineEvent>>()
                {
                    locked.queue.push(ev);
                }
                let mut batch = vec![];
                while let Some(ev) = locked.queue.peek() {
                    if ev.at > transport_time {
                        break;
                    }
                    batch.push(locked.queue.pop().unwrap().event);
                }
                for ev in batch {
                    // Keeping actual value to resume playback with sustain enabled if necessary.
                    // Otherwise, it will only be active after next explicit change.
                    if let LiveEvent::Midi {
                        message: MidiMessage::Controller { controller, .. },
                        ..
                    } = ev
                    {
                        if controller == MIDI_CC_SUSTAIN_ID {
                            locked.current_sustain = Some(ev.to_static());
                        }
                    }

                    locked.process(ev);
                }
            }
        });
        engine
    }

    fn update_track_time(&mut self) {
        self.running_at = Instant::now().duration_since(self.reset_at).as_micros() as Time;
        self.status_receiver
            .as_mut()
            .map(|recv| recv(StatusEvent::Time(self.running_at)));
    }

    pub fn seek(&mut self, at: Time) {
        for s in self.sources.iter_mut() {
            s.seek(&at);
        }
        self.running_at = at;
        self.update_realtime();
        self.update_track_time();
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused {
            self.update_realtime();
        }
        self.command_sender
            .send(Box::new(|engine| {
                if engine.paused {
                    // Mute ongoing notes before clearing.
                    engine.queue.clear();
                    for key in 0..u7::max_value().into() {
                        engine.process(LiveEvent::Midi {
                            channel: MIDI_CHANNEL.into(),
                            message: NoteOff {
                                key: key.into(),
                                vel: 64.into(),
                            },
                        });
                    }
                    engine.process(LiveEvent::Midi {
                        channel: MIDI_CHANNEL.into(),
                        message: MidiMessage::Controller {
                            controller: MIDI_CC_SUSTAIN_ID.into(),
                            value: 0.into(),
                        },
                    });
                } else if let Some(sustain) = engine.current_sustain {
                    engine.queue.push(EngineEvent {
                        at: engine.running_at,
                        event: sustain,
                    });
                }
            }))
            .unwrap();
    }

    /// Stop all sounds.
    pub fn reset(&mut self) {
        self.paused = true;
    }

    pub fn update_realtime(&mut self) {
        self.reset_at = Instant::now() - Duration::from_micros(self.running_at as u64);
    }

    pub fn add(&mut self, source: Box<EventSourceHandle>) {
        self.sources.push(source);
    }

    /// Process the event immediately.
    pub fn process(&mut self, event: LiveEvent) {
        let mut midi_buf = vec![];
        event.write(&mut midi_buf).unwrap();
        self.midi_output
            .send(&midi_buf)
            .expect("send output MIDI event");
    }

    pub fn set_status_receiver(&mut self, receiver: Option<Box<StatusEventReceiver>>) {
        self.status_receiver = receiver;
    }
}

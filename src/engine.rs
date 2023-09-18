use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::{Arc, mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use midly::live::LiveEvent;
use midly::MidiMessage;
use vst::event::Event;
use vst::plugin::Plugin;

use crate::midi_vst::Vst;

/// uSecs from the start.
pub type TransportTime = u64;

/** Event that is produced by engine. */
#[derive(Clone, Debug)]
pub enum StatusEvent {
    TransportTime(TransportTime),
}

/// A sound event to be rendered by the engine at given time.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct EngineEvent {
    pub at: TransportTime,
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

pub type EngineCommand = dyn FnOnce(&mut Engine) + Send;

impl PartialOrd for EngineEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub trait EventSource {
    /** Return false when no events will be produced from the source anymore.
    Use this to detach it from the engine. */
    fn is_running(&self) -> bool;
    /** Reset current source's time to this moment. */
    fn seek(&mut self, at: &TransportTime);
    /** The next event to be played at the instant. On subsequent
    calls instants must not decrease unless a reset call sets another time. */
    fn next(&mut self, at: &TransportTime, queue: &mut BinaryHeap<EngineEvent>);
}

type EventSourceHandle = dyn EventSource + Send;

pub struct Engine {
    vst: Vst,
    sources: Vec<Box<EventSourceHandle>>,
    running_at: TransportTime,
    reset_at: Instant,
    paused: bool,
    status_receiver: Option<Box<StatusEventReceiver>>,
    commands: mpsc::Receiver<Box<EngineCommand>>,
}

impl Engine {
    pub fn new(vst: Vst, commands: mpsc::Receiver<Box<EngineCommand>>) -> Engine {
        Engine {
            vst,
            sources: Vec::new(),
            running_at: 0,
            reset_at: Instant::now(),
            paused: false,
            status_receiver: None,
            commands,
        }
    }

    pub fn start(self) -> Arc<Mutex<Engine>> {
        let engine = Arc::new(Mutex::new(self));
        let engine2 = engine.clone();
        thread::spawn(move || {
            engine2.lock().unwrap().seek(0);
            let mut queue: BinaryHeap<EngineEvent> = BinaryHeap::new();
            loop {
                thread::sleep(Duration::from_micros(3_000));
                let lock = engine2.lock();
                if let Err(_) = lock {
                    continue;
                }
                let mut locked = lock.unwrap();
                let pending_commands: Vec<Box<EngineCommand>> =
                    locked.commands.try_iter().collect();
                for command in pending_commands {
                    command(&mut locked);
                }
                if locked.paused {
                    // Mute ongoing notes before clearing.
                    // TODO Some sounds may still continue on sustain. Need a panic button.
                    // TODO Avoid doing this at every iteration?
                    for ev in queue.iter() {
                        if let LiveEvent::Midi {
                            message,
                            channel: _,
                        } = ev.event
                        {
                            if let MidiMessage::NoteOff { .. } = message {
                                locked.process(smf_to_vst(ev.event));
                            }
                        }
                    }
                    queue.clear();
                    continue;
                };
                locked.sources.retain(|s| s.is_running());
                Self::update_track_time(&mut locked);
                let transport_time = locked.running_at;
                for s in locked.sources.iter_mut() {
                    // Alternatively could pass a small pre-allocated array to hold the output events.
                    s.next(&transport_time, &mut queue);
                }
                let mut batch = vec![];
                while let Some(ev) = queue.peek() {
                    if ev.at > transport_time {
                        break;
                    }
                    batch.push(queue.pop().unwrap().event);
                }
                for ev in batch {
                    locked.process(smf_to_vst(ev));
                }
            }
        });
        engine
    }

    fn update_track_time(&mut self) {
        self.running_at = Instant::now().duration_since(self.reset_at).as_micros() as u64;
        self.status_receiver
            .as_mut()
            .map(|recv| recv(StatusEvent::TransportTime(self.running_at)));
    }

    pub fn seek(&mut self, at: TransportTime) {
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
    }

    /// Stop all sounds.
    pub fn reset(&mut self) {
        self.paused = true;
        // TODO Implement.
        // self.vst.host.lock().unwrap().idle(); // This SIGSEVs. Use LV2 instead?
    }

    pub fn update_realtime(&mut self) {
        self.reset_at = Instant::now() - Duration::from_micros(self.running_at);
    }

    pub fn add(&mut self, source: Box<EventSourceHandle>) {
        self.sources.push(source);
    }

    /// Process the event immediately
    pub fn process(&self, event: Event) {
        let events_list = [event];
        let mut events_buffer = vst::buffer::SendEventBuffer::new(events_list.len());
        events_buffer.store_events(events_list);
        let mut plugin = self.vst.plugin.lock().unwrap();
        plugin.process_events(events_buffer.events());
    }

    pub fn set_status_receiver(&mut self, receiver: Option<Box<StatusEventReceiver>>) {
        self.status_receiver = receiver;
    }
}

fn smf_to_vst(event: LiveEvent<'static>) -> Event<'static> {
    let mut ev_buf = Vec::new();
    event
        .write(&mut ev_buf)
        .expect("The live event should be writable.");
    Event::Midi(vst::event::MidiEvent {
        data: ev_buf.try_into().unwrap(),
        delta_frames: 0,
        live: true,
        note_length: None,
        note_offset: None,
        detune: 0,
        note_off_velocity: 0,
    })
}

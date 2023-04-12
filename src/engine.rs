use std::thread;
use std::time::{Duration, Instant};
use vst::event::Event;
use crate::midi_vst::Vst;
use std::sync::{Arc, Mutex};
use midly::live::LiveEvent;
use vst::plugin::Plugin;

/// An event to be rendered by the engine at given time
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct EngineEvent {
    pub event: LiveEvent<'static>,
}

/// Micros from the start.
pub type TransportTime = u64;

pub trait EventSource {
    /** Return false when no new events will be produced from the source. */
    fn is_running(&self) -> bool;
    /** Reset current source's time to this moment. */
    fn reset(&mut self, at: &TransportTime);
    /** The next event to be played at the instant.
                             On subsequent calls instants must not decrease unless a reset call sets another time.
     */
    fn next(&mut self, at: &TransportTime) -> Option<EngineEvent>;
}

type EventSourceHandle = dyn EventSource + Send;

pub struct Engine {
    vst: Vst,
    sources: Vec<Box<EventSourceHandle>>,
    running_at: TransportTime,
    reset_at: Instant,
}

impl Engine {
    // TODO (scheduling) some transport controls. Maybe: pause/unpause - pause processing events, reset - clear queue.

    pub fn new(vst: Vst) -> Engine {
        Engine {
            vst,
            sources: Vec::new(),
            running_at: 0,
            reset_at: Instant::now(),
        }
    }

    pub fn start(self) -> Arc<Mutex<Engine>> {
        let engine = Arc::new(Mutex::new(self));
        let engine2 = engine.clone();
        thread::spawn(move || {
            engine2.lock().unwrap().reset(0);
            loop {
                thread::sleep(Duration::from_micros(1_000));
                let mut locked = engine2.lock().unwrap();
                locked.sources.retain(|s| s.is_running());
                let transport_time = locked.running_at.to_owned() + Instant::now()
                    .duration_since(locked.reset_at.to_owned())
                    .as_micros() as u64;
                let mut batch = vec![];
                for s in locked.sources.iter_mut() {
                    while let Some(ev) = s.next(&transport_time) {
                        batch.push(smf_to_vst(ev.event));
                    }
                }
                for ev in batch.iter() {
                    locked.process(*ev);
                }
            }
        });
        engine
    }

    pub fn reset(&mut self, at: TransportTime) {
        self.running_at = at;
        self.reset_at = Instant::now();
        for s in self.sources.iter_mut() {
            s.reset(&at);
        }
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
}

fn smf_to_vst(event: midly::live::LiveEvent<'static>) -> Event<'static> {
    let mut ev_buf = Vec::new();
    event.write(&mut ev_buf)
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

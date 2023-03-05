use std::ops::Deref;
use std::thread;
use std::time::{Duration, Instant};
use vst::event::Event;
use crate::midi_vst::Vst;
use vst::host::{Host, HostBuffer, PluginInstance};
use std::sync::{Arc, Mutex};
use midly::{MidiMessage, TrackEvent};
use midly::live::LiveEvent;
use vst::plugin::Plugin;

/// An event to be rendered by the engine at given time
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct EngineEvent {
    pub event: LiveEvent<'static>,
}

pub trait EventSource {
    /** Return false when no new events will be produced from the source. */
    fn is_running(&self) -> bool;
    /** The next event to be played at the instant. */
    fn next(&mut self, at: &Instant) -> Option<EngineEvent>;
}

type EventSourceHandle = dyn EventSource + Send;

pub struct Engine {
    vst: Vst,
    sources: Arc<Mutex<Vec<Box<EventSourceHandle>>>>,
}

impl Engine {
    // TODO (scheduling) some transport controls. Maybe: pause/unpause - pause processing events, reset - clear queue.
    // TODO (scheduling) send - add an event to the queue (should wake if the new event is earlier than all others)

    pub fn new(vst: Vst) -> Engine {
        Engine { vst, sources: Arc::new(Mutex::new(Vec::new())) }
    }

    pub fn start(self) -> Arc<Engine> {
        let engine = Arc::new(self);
        let engine2 = engine.clone();
        thread::spawn(move || {
            loop {
                let mut sources = engine2.sources.lock().unwrap();
                thread::sleep(Duration::from_micros(300));
                sources.retain(|s| s.is_running());
                let now = Instant::now();
                for s in sources.iter_mut() {
                    /* TODO (implementation) A source may have more than one event for a given instant.
                        Render all events that are currently available. */
                    if let Some(e) = s.next(&now) {
                        engine2.process(smf_to_vst(e.event));
                    }
                }
            }
        });
        engine
    }

    pub fn add(&self, source: Box<EventSourceHandle>) {
        self.sources.lock().unwrap().push(source);
    }

    /// Process the event at specified moment
    // pub fn schedule(&self, event: &EngineEvent) {
    //     todo!("");
    // }

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

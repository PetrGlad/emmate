use std::ops::Deref;
use std::thread;
use std::time::Duration;
// use std::collections::BinaryHeap;
use vst::event::Event;
use crate::midi_vst::Vst;
use vst::host::{Host, HostBuffer, PluginInstance};
use std::sync::{Arc, Mutex};
use midly::{MidiMessage, TrackEvent};
use vst::plugin::Plugin;

/// An event to be rendered by the engine at given time
// #[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
// pub struct EngineEvent {
//     /// Scheduled moment in microseconds from now.
//     pub dt: u32,
//     pub event: midly::MidiMessage,
// }
pub type EngineEvent = TrackEvent<'static>;


pub type MidiSource = dyn Iterator<Item=EngineEvent> + Send;

pub struct Engine {
    vst: Vst,
    sources: Arc<Mutex<Vec<Box<MidiSource>>>>,
}

// impl EngineEvent {
//     // TODO Ord, PartialOrd by timestamp
//     // TODO new() from sequencer midi event
// }

impl Engine {
    // TODO some transport controls. Maybe: pause/unpause - pause processing events, reset - clear queue.
    // TODO send - add an event to the queue (should wake if the new event is earlier than all others)

    pub fn new(vst: Vst) -> Engine {
        Engine { vst, sources: Arc::new(Mutex::new(Vec::new())) }
    }

    pub fn start(self) -> Arc<Engine> {
        let engine = Arc::new(self);
        let engine2 = engine.clone();
        thread::spawn(move || {
            loop {
                let mut sources = engine2.sources.lock().unwrap();
                // TODO Multi-source would ask each source for a new event and play the earliest one.
                // TODO Immediate schedule should interrupt sleep.
                assert!(sources.len() <= 1, "Only single source is supported in the prototype.");
                for s in sources.iter_mut() {
                    let ev = s.next();
                    match ev {
                        Some(e) => {
                            thread::sleep(Duration::from_micros(e.dt as u64));
                            engine2.process(smf_to_vst(e.event));
                        }
                        None => {
                            // TODO Remove the source, keep processing.
                            println!("The MIDI source is completely processed, stopping engine.");
                            return;
                        }
                    }
                }
                // TODO XXX Relieving contention, in a full implementation should not be needed.
                thread::sleep(Duration::from_millis(5));
            }
        });
        engine
    }

    pub fn add(&self, source: Box<MidiSource>) {
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

fn smf_to_vst<'a>(smf_midi_event: MidiMessage) -> Event<'a> {
    todo!();
    // TODO encode the event
    let mut ev_buf= [0u8;0];
    // smf_midi_event.write();
    Event::Midi(vst::event::MidiEvent {
        data: ev_buf,
        delta_frames: 0,
        live: true,
        note_length: None,
        note_offset: None,
        detune: 0,
        note_off_velocity: 0,
    })
}

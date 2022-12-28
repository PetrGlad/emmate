use std::thread;
use std::time::Duration;
// use std::collections::BinaryHeap;
use vst::event::Event;
use crate::midi_vst::Vst;
use vst::host::{Host, HostBuffer, PluginInstance};
use std::sync::{Arc, Mutex};
use vst::plugin::Plugin;

/////////////////////////////////////////////////////////////////////////////////////////////
//>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>


// use std::{error, primitive, result};
// use std::borrow::BorrowMut;
// use std::ffi::CString;
// use std::io::{BufReader, BufWriter, stdin};
// use std::ops::DerefMut;
// use std::path::Path;
// use std::process::exit;
// use std::thread::{park_timeout, sleep};

// use alsa::Direction;
// use cpal::{BufferSize, ChannelCount, SampleFormat, SampleRate, StreamConfig, SupportedBufferSize, SupportedStreamConfig};
// use cpal::SampleFormat::F32;
// use cpal::SupportedBufferSize::Range;
// use iced::{
//     Alignment, button, Button, Column, Element, Sandbox, Settings, Text,
// };
// use midir::MidiInput;
// use midly::{Format, MidiMessage, Timing, TrackEvent, TrackEventKind};
// use midly::io::Cursor;
// use midly::MidiMessage::NoteOn;
// use midly::TrackEventKind::Midi;
// use rodio::{cpal, OutputStream, Source};
// use rodio::source::SineWave;
// use rodio::source::TakeDuration;
// use vst::api::Events;

// use wav::BitDepth;
// use crate::midi_vst::{OutputSource};
// use vst::event::{MidiEvent};
// use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

//<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<
/////////////////////////////////////////////////////////////////////////////////////////////


/// An event to be rendered by the engine at given time
pub struct EngineEvent<'a> {
    /// Scheduled moment in microseconds.
    /// TODO Since beginning of the score? What for real-time ones then? Or have an option score-time/real-time
    at: u64,
    midi_event: Event<'a>,
}

type MidiSource<'a> = dyn Iterator<Item=&'a EngineEvent<'a>>;

pub struct Engine/*<'a>*/ {
    vst: Vst,
    // sources: Vec<&'a Arc<Mutex<MidiSource<'a>>>>,
}

impl<'a> EngineEvent<'a> {
    // TODO Ord, PartialOrd by timestamp
    // TODO new() from sequencer midi event
}

impl/*<'a>*/ Engine/*<'a>*/ {
    // TODO some transport controls. Maybe: pause/unpause - pause processing events, reset - clear queue.
    // TODO send - add an event to the queue (should wake if the new event is earlier than all others)

    pub fn new(vst: Vst) -> Engine/*<'a>*/ {
        Engine { vst/*, sources: Vec::new()*/ }
    }

    pub fn start(&mut self) {
        // thread::spawn(|| {
        //     // FIXME Implement
        //     for s in self.sources.lock() {
        //         // println!("hi number {} from the spawned thread", s.next());
        //         println!("hi from the engine");
        //         thread::sleep(Duration::from_millis(6789));
        //     }
        // });
    }

    /// Process the event at specified moment
    // pub fn schedule(&self, event: &EngineEvent) {
    //     todo!("");
    // }

    /// Process the event immediately
    pub fn process(&mut self, event: Event) {
        let events_list = [event];
        let mut events_buffer = vst::buffer::SendEventBuffer::new(events_list.len());
        events_buffer.store_events(events_list);
        let mut plugin = self.vst.plugin.lock().unwrap();
        plugin.process_events(events_buffer.events());
    }
}

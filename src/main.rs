mod midi_vst;
mod midi;
mod engine;
mod events;

use std::{error, primitive, result, thread};
use std::borrow::BorrowMut;
use std::ffi::CString;
use std::io::{BufReader, BufWriter, stdin};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::thread::{park_timeout, sleep};
use std::time::Duration;

use alsa::Direction;
use cpal::{BufferSize, ChannelCount, SampleFormat, SampleRate, StreamConfig, SupportedBufferSize, SupportedStreamConfig};
use cpal::SampleFormat::F32;
use cpal::SupportedBufferSize::Range;
use iced::{Alignment, widget::button, widget::Button, widget::Column, Element, Sandbox, Settings, widget::Text, widget};
use midir::{MidiInput, MidiInputConnection};
use midly::{Format, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use midly::io::Cursor;
use midly::MidiMessage::NoteOn;
use midly::TrackEventKind::Midi;
use rodio::{cpal, OutputStream, Source};
use rodio::source::SineWave;
use rodio::source::TakeDuration;
use vst::api::Events;
use vst::event::Event;
use vst::host::{Host, HostBuffer, PluginInstance};
use vst::plugin::Plugin;
use wav::BitDepth;
use crate::midi_vst::{OutputSource, Vst};
use vst::event::{MidiEvent};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::engine::Engine;
use crate::midi::SmfSource;

pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }
    // Stream reference keeps it open.
    let (_stream, mut engine) = setup_audio_engine();

    { // Play MIDI from an SMD file.
        let smf_data = std::fs::read("yellow.mid").unwrap();
        let smf_midi_source = SmfSource::new(smf_data);
        engine.lock().unwrap().add(Box::new(smf_midi_source));
    }

    let mut midi_inputs = vec![]; // Keeps inputs open
    midi_inputs.push(midi_keyboard_input("Digital Piano", &mut engine));
    midi_inputs.push(midi_keyboard_input("XPIANOGT", &mut engine));
    midi_inputs.push(midi_keyboard_input("MPK mini 3", &mut engine));

    // GUI example
    Ed::run(Settings::default()).unwrap()
}

fn setup_audio_engine() -> (OutputStream, Arc<Mutex<Engine>>) {
    let buffer_size = 256;
    let audio_host = cpal::default_host();
    let out_device = audio_host.default_output_device().unwrap();
    println!("Default output device: {:?}", out_device.name());
    let out_conf = out_device.default_output_config().unwrap();
    println!("Default output config: {:?}", out_conf);
    assert!(out_conf.sample_format() == F32);
    let sample_format = F32; // To use with vst.
    let out_conf = StreamConfig {
        channels: out_conf.channels(),
        sample_rate: out_conf.sample_rate(),
        buffer_size: BufferSize::Fixed(buffer_size),
    };
    println!("Output config: {:?}", out_conf);
    let (stream, stream_handle) =
        rodio::OutputStream::try_from_config(&out_device, &out_conf, &sample_format).unwrap();
    let vst = Vst::init(&out_conf.sample_rate, &buffer_size);
    stream_handle.play_raw(OutputSource::new(&vst, &buffer_size)).unwrap();
    let engine = Engine::new( vst);
    (stream, engine.start())
}

fn midi_keyboard_input(name_prefix: &str, engine: &mut Arc<Mutex<Engine>>) -> Option<MidiInputConnection<()>> {
    let input = MidiInput::new("midir").unwrap();
    let mut port_idx = None;
    println!("Midi input ports:");
    let ports = input.ports();
    for (i, port) in ports.iter().enumerate() {
        let name = input.port_name(&port).unwrap();
        println!("\t{}", name);
        if name.starts_with(name_prefix) {
            port_idx = Some(i);
            println!("Selected MIDI input: '{}'", name);
            break;
        }
    }

    if port_idx == None {
        println!("WARN No midi input selected.");
        return None;
    }
    let port = ports.get(port_idx.unwrap())
        .unwrap();
    let seq_engine = engine.clone();
    Some(input.connect(
        &port,
        "midi-input",
        move |t, ev, _data| {
            println!("MIDI event: {} {:?} {}", t, ev, ev.len());
            if ev[0] == 254 { return; }
            let mut ev_buf = [0u8; 3];
            for (i, x) in ev.iter().enumerate() {
                ev_buf[i] = *x;
            }
            let note = Event::Midi(MidiEvent {
                data: ev_buf,
                delta_frames: 0,
                live: true,
                note_length: None,
                note_offset: None,
                detune: 0,
                note_off_velocity: 0,
            });
            seq_engine.lock().unwrap().process(note);
        },
        (),
    ).unwrap())
}

/*
  Using https://github.com/iced-rs/iced/blob/0.3/examples/counter/src/main.rs
  as a stub implementation for starters.
*/

#[derive(Default)]
struct Ed {
    value: i32,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    IncrementPressed,
    DecrementPressed,
}

impl Sandbox for Ed {
    type Message = Message;

    fn new() -> Self {
        Self::default()
    }

    fn title(&self) -> String {
        String::from("Midired")
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::IncrementPressed => {
                self.value += 1;
            }
            Message::DecrementPressed => {
                self.value -= 1;
            }
        }
    }

    fn view(&self) -> Element<Message> {
        Column::new()
            .padding(20)
            .align_items(Alignment::Start)
            .push(
                Button::new(Text::new("Increment"))
                    .on_press(Message::IncrementPressed),
            )
            .push(Text::new(self.value.to_string()).size(50))
            .push(
                Button::new(Text::new("Decrement"))
                    .on_press(Message::DecrementPressed),
            )
            .into()
    }
}

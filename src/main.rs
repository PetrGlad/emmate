/*
  Using https://github.com/iced-rs/iced/blob/0.3/examples/counter/src/main.rs
  as a stub implementation for starters.
*/
mod midi_vst;
mod midi;

use std::{error, result, thread};
use std::borrow::BorrowMut;
use std::ffi::CString;
use std::io::{BufReader, BufWriter, stdin};
use std::ops::DerefMut;
use std::path::Path;
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

use alsa::Direction;
use iced::{
    Alignment, button, Button, Column, Element, Sandbox, Settings, Text,
};
use midir::MidiInput;
use midly::{TrackEvent, TrackEventKind};
use midly::io::Cursor;
use midly::MidiMessage::NoteOn;
use midly::TrackEventKind::Midi;
use rodio::OutputStream;
use rodio::source::SineWave;
use vst::api::Events;
use vst::host::{Host, HostBuffer};
use vst::plugin::Plugin;
use wav::BitDepth;
use crate::midi_vst::{OutputSource, Vst};

pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }

    {
        let input = MidiInput::new("midir").unwrap();
        let mut port_idx = None;
        println!("Midi input ports:");
        let ports = input.ports();
        for (i, port) in ports.iter().enumerate() {
            let name = input.port_name(&port).unwrap();
            println!("\t{}", name);
            if name.starts_with("Digital Piano") {
                port_idx = Some(i);
                println!("Selected MIDI input: '{}'", name);
                break;
            }
        }
        let port = ports.get(port_idx.unwrap()).unwrap();
        let conn = input.connect(
            &port,
            "midi-input",
            |t, ev, _data| println!("MIDI event: {} {:?} {}", t, ev, ev.len()),
            (),
        ).unwrap();

        println!("Connection open, reading MIDI input");
        let mut input = String::new();
        input.clear();
        stdin().read_line(&mut input).unwrap(); // wait for next enter key press
        conn.close();
    }
    {
        // Load VST example
        use vst::api::{Events, Supported};
        use vst::event::{Event, MidiEvent};
        use vst::host::{Host, HostBuffer, PluginLoader};

        let note_event = midly::live::LiveEvent::Midi {
            channel: 1.into(),
            message: midly::MidiMessage::NoteOn {
                key: 50.into(),
                vel: 80.into(),
            },
        };
        let mut track_event_buf = [0u8; 3];
        let mut cursor = midly::io::Cursor::new(&mut track_event_buf);
        note_event.write(&mut cursor).unwrap();
        println!("Event bytes {:?}\n", track_event_buf);
        let note = Event::Midi(MidiEvent {
            data: track_event_buf,
            delta_frames: 0,
            live: true,
            note_length: None,
            note_offset: None,
            detune: 0,
            note_off_velocity: 0,
        });
        let events_list = [note];

        let mut vst = Vst::init();
        let mut events_buffer = vst::buffer::SendEventBuffer::new(events_list.len());
        events_buffer.store_events(events_list);
        // XXX See https://github.com/RustAudio/vst-rs/pull/160.
        // Do not know yet how to make a plugin reference available in a Host implementation.
        // events_buffer.send_events(events_list, &mut *(VST.host.lock().unwrap()));
        let plugin_holder = vst.plugin.clone();
        {
            println!("Processing events.");
            let mut plugin = plugin_holder.lock().unwrap();
            plugin.process_events(events_buffer.events());
        }
        // VST.plugin.process(&mut audio_buffer);
        // for out in &outputs {
        //     println!("Output {:?}", out);
        // }
        //
        //
        // {
        //     // I want to hear it at least somehow
        //     use std::fs::File;
        //
        //     // let mut inp_file = File::open(Path::new("data/sine.wav"))?;
        //     // let (header, data) = wav::read(&mut inp_file)?;
        //     let wav_header = wav::Header::new(wav::WAV_FORMAT_IEEE_FLOAT, 2, 48000, 32);
        //
        //     let mut out_file = File::create(Path::new("output.wav")).unwrap();
        //     // wav::write(wav_header, BitDepth::ThirtyTwoFloat &mut out_file).unwrap();
        //     let mut pcm_data = vec![];
        //     for _i in 1..20 {
        //         plugin.process(&mut audio_buffer);
        //         pcm_data.append(&mut outputs[0].to_vec());
        //     }
        //     let wav_data = wav::BitDepth::ThirtyTwoFloat(pcm_data.to_owned());
        //     wav::write(wav_header, &wav_data, &mut out_file).unwrap();
        //     drop(out_file);
        // }
        // TODO Output the sound to default audio device using rodio
        {
            println!("Streaming sound (hopefully).");
            let (_stream, stream_handle) = rodio::OutputStream::try_default().unwrap();

            let file = std::fs::File::open("output.wav").unwrap();
            let beep_sink = stream_handle.play_once(BufReader::new(file)).unwrap();
            beep_sink.set_volume(0.5);

            // This seems like a good example of generative audio source https://github.com/RustAudio/rodio/blob/master/src/source/sine.rs
            // stream_handle.play_raw(SineWave::new(1000.0)).unwrap();
            stream_handle.play_raw(OutputSource::new(&vst)).unwrap();

            thread::sleep(Duration::from_millis(3000));
        }

        // println!("Closing host instance.");
        // drop(instance);
    }

    { // GUI example
        Ed::run(Settings {
            antialiasing: true,
            ..Settings::default()
        }).unwrap()
    }
}

#[derive(Default)]
struct Ed {
    value: i32,
    increment_button: button::State,
    decrement_button: button::State,
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

    fn view(&mut self) -> Element<Message> {
        Column::new()
            .padding(20)
            .align_items(Alignment::Start)
            .push(
                Button::new(&mut self.increment_button, Text::new("Increment"))
                    .on_press(Message::IncrementPressed),
            )
            .push(Text::new(self.value.to_string()).size(50))
            .push(
                Button::new(&mut self.decrement_button, Text::new("Decrement"))
                    .on_press(Message::DecrementPressed),
            )
            .into()
    }
}
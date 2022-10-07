/*
  Using https://github.com/iced-rs/iced/blob/0.3/examples/counter/src/main.rs
  as a stub implementation for starters.
*/
mod midi_vst;
mod midi;

use std::{error, result};
use std::borrow::BorrowMut;
use std::ffi::CString;
use std::io::BufWriter;
use std::ops::DerefMut;
use std::path::Path;
use std::process::exit;
use std::sync::{Arc, Mutex};

use alsa::Direction;
use iced::{
    Alignment, button, Button, Column, Element, Sandbox, Settings, Text,
};
use midly::{TrackEvent, TrackEventKind};
use midly::io::Cursor;
use midly::MidiMessage::NoteOn;
use midly::TrackEventKind::Midi;
use vst::host::{Host, HostBuffer};
use vst::plugin::Plugin;
use crate::midi_vst::Vst;


pub fn main() {
    {
        // Load VST example
        use vst::api::{Events, Supported};
        use vst::event::{Event, MidiEvent};
        use vst::host::{Host, HostBuffer, PluginLoader};

        let note_event = midly::live::LiveEvent::Midi {
            channel: 1.into(),
            message: midly::MidiMessage::NoteOn {
                key: 40.into(),
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

        let info = vst.plugin.get_info();
        let output_count = info.outputs as usize;
        let input_count = info.inputs as usize;
        let buf_size = 1 << 11;
        let inputs = vec![vec![0.0; buf_size]; input_count];
        let mut outputs = vec![vec![0.0; buf_size]; output_count];
        let mut host_buffer: HostBuffer<f32> = HostBuffer::new(input_count, output_count);
        let mut audio_buffer = host_buffer.bind(&inputs, &mut outputs);

        vst.plugin.start_process();
        let mut events_buffer = vst::buffer::SendEventBuffer::new(events_list.len());
        events_buffer.send_events(events_list, &mut *(vst.host.lock().unwrap()));
        println!("Processing events.");
        vst.plugin.process(&mut audio_buffer);
        for out in &outputs {
            println!("Output {:?}\n", out);
        }

        // TODO Output the sound to default audio device using rodio

        {
            // I want to hear it at least somehow
            use std::fs::File;

            // let mut inp_file = File::open(Path::new("data/sine.wav"))?;
            // let (header, data) = wav::read(&mut inp_file)?;
            let wav_header = wav::Header::new(wav::WAV_FORMAT_IEEE_FLOAT, 1, 48000, 32);

            let wav_data = wav::BitDepth::ThirtyTwoFloat(outputs[0].to_owned());
            let mut out_file = File::create(Path::new("output.wav")).unwrap();
            wav::write(wav_header, &wav_data, &mut out_file).unwrap();
        }
        {
            // TODO Actually output sound:
            // Have the VST to produce a sequence of buffers on demand via an f32 iterator, send them to Rodio
            // This seems like a good example of generative audio source https://github.com/RustAudio/rodio/blob/master/src/source/sine.rs
        }

        // println!("Closing host instance.");
        // drop(instance);
    }

    // { // GUI example
    //     Ed::run(Settings {
    //         antialiasing: true,
    //         ..Settings::default()
    //     }).unwrap()
    // }
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
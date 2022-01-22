/*
  Using https://github.com/iced-rs/iced/blob/0.3/examples/counter/src/main.rs
  as a stub implementation for starters.
*/
use std::{error, result};
use std::ffi::CString;
use std::io::BufWriter;
use std::path::Path;
use std::process::exit;
use std::sync::{Arc, Mutex};

use alsa::Direction;
use iced::{
    Align, button, Button, Column, Element, Sandbox, Settings, Text,
};
use midly::{TrackEvent, TrackEventKind};
use midly::io::Cursor;
use midly::MidiMessage::NoteOn;
use midly::TrackEventKind::Midi;
use vst::plugin::Plugin;

mod midi_vst;

pub fn main() {
    // { // Use ALSA to read midi events
    //
    //     // TODO: Replace ALSA with midir for reading events. See https://docs.rs/midir/0.7.0/midir/struct.MidiInput.html
    //
    //     // Diagnostics commands
    //     //   amidi --list-devices
    //     //   aseqdump --list
    //     //   aseqdump --port='24:0'
    //
    //     let seq = alsa::seq::Seq::open(None, Some(Direction::Capture), false)
    //         .expect("Cannot open MIDI sequencer.");
    //
    //     for cl in alsa::seq::ClientIter::new(&seq) {
    //         println!("Found a client {:?}", &cl);
    //     }
    //
    //     let mut subscription = alsa::seq::PortSubscribe::empty().unwrap();
    //     subscription.set_sender(alsa::seq::Addr { client: 24, port: 0 }); // Note: hardcoded. // TODO Use a client from available list
    //     // subscription.set_sender(alsa::seq::Addr::system_timer());
    //     let input_port = seq.create_simple_port(
    //         &CString::new("midi input").unwrap(),
    //         alsa::seq::PortCap::WRITE | alsa::seq::PortCap::SUBS_WRITE,
    //         alsa::seq::PortType::MIDI_GENERIC).unwrap();
    //     subscription.set_dest(alsa::seq::Addr {
    //         client: seq.client_id().unwrap(),
    //         port: input_port,
    //     });
    //     subscription.set_time_update(false);
    //     subscription.set_time_real(true); // Allows to event.get_tick
    //
    //     seq.subscribe_port(&subscription).unwrap();
    //     let mut midi_input = seq.input();
    //     loop {
    //         let midi_event = midi_input.event_input().unwrap();
    //         println!("Got MIDI event {:?}", midi_event);
    //         if midi_event.get_type() == alsa::seq::EventType::Noteon {
    //             let ev_data: alsa::seq::EvNote = midi_event.get_data().unwrap();
    //             println!("Got NOTE ON event {:?}", &ev_data);
    //             break;
    //         }
    //     }
    // }

    { // MIDI load/modify example
        let data = std::fs::read("yellow.mid").unwrap();
        // Parse the raw bytes
        let mut smf = midly::Smf::parse(&data).unwrap();
        // Use the information
        println!("midi file has {} tracks, format is {:?}.", smf.tracks.len(), smf.header.format);
        let track = smf.tracks.get_mut(0).unwrap();

        println!("The 1st track is {:#?}", &track);

        // Try to do some modifications
        let mut i = 0;
        while i < track.len() {
            let skip = match track[i].kind {
                TrackEventKind::Meta(_) => true,
                _ => false
            };
            if skip {
                track.remove(i);
            } else {
                i += 1;
            }
        }

        smf.save("rewritten.mid").unwrap();
    }

    {
        // Load VST example
        use vst::api::{Events, Supported};
        use vst::event::{Event, MidiEvent};
        use vst::host::{Host, HostBuffer, PluginLoader};

        use crate::midi_vst::VstHost;

        let mut plugin = VstHost::init();

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

        let mut instance = plugin.instance().unwrap();
        let info = instance.get_info();
        let input_count = info.inputs as usize;
        let output_count = info.outputs as usize;
        let mut host_buffer: HostBuffer<f32> = HostBuffer::new(input_count, output_count);
        let buf_size = 1 << 14;
        let inputs = vec![vec![0.0; buf_size]; input_count];
        let mut outputs = vec![vec![0.0; buf_size]; output_count];
        let mut audio_buffer = host_buffer.bind(&inputs, &mut outputs);

        // instance.suspend(); // Can only set these parameters in suspended state.
        // instance.set_sample_rate(48000f32);
        // instance.set_block_size(128);

        instance.resume();
        instance.start_process();

        let mut events_buffer = vst::buffer::SendEventBuffer::new(1);
        events_buffer.send_events_to_plugin([note], &mut instance);
        instance.process(&mut audio_buffer);

        for out in &outputs {
            println!("Output {:?}\n", out);
        }
        // TODO Output the sound to default audio device using rodio

        {
            // I want to hear it
            use std::fs::File;

            // let mut inp_file = File::open(Path::new("data/sine.wav"))?;
            // let (header, data) = wav::read(&mut inp_file)?;
            let wav_header = wav::Header::new(
                wav::WAV_FORMAT_IEEE_FLOAT, 1, 48000, 32);

            let wav_data = wav::BitDepth::ThirtyTwoFloat(outputs[0].to_owned());
            let mut out_file = File::create(Path::new("output.wav")).unwrap();
            wav::write(wav_header, &wav_data, &mut out_file).unwrap();
        }
        {
            // TDDO Actually output sound:
            // Produce a sequence of buffers that are produced by VST on demand, send them to Rodio
        }

        // println!("Closing instance...");
        // Close the instance. This is not necessary as the instance is shut down when
        // it is dropped as it goes out of scope.
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
            .align_items(Align::Start)
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
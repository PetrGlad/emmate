mod midi_vst;
mod midi;
mod engine;

use std::{error, primitive, result, thread};
use std::borrow::BorrowMut;
use std::ffi::CString;
use std::io::{BufReader, BufWriter, stdin};
use std::ops::DerefMut;
use std::path::Path;
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::thread::{park_timeout, sleep};
use std::time::Duration;

use alsa::Direction;
use cpal::{BufferSize, ChannelCount, SampleFormat, SampleRate, StreamConfig, SupportedBufferSize, SupportedStreamConfig};
use cpal::SampleFormat::F32;
use cpal::SupportedBufferSize::Range;
use iced::{
    Alignment, button, Button, Column, Element, Sandbox, Settings, Text,
};
use midir::MidiInput;
use midly::{Format, MidiMessage, Timing, TrackEvent, TrackEventKind};
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


pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }
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
    let (_stream, stream_handle) =
        rodio::OutputStream::try_from_config(&out_device, &out_conf, &sample_format).unwrap();
    // let (_stream, stream_handle) = rodio::OutputStream::try_default().unwrap();

    {
        let vst = Vst::init(&out_conf.sample_rate, &buffer_size);
        stream_handle.play_raw(OutputSource::new(&vst, &buffer_size)).unwrap();
        let engine = Arc::new(Mutex::new(Engine::new(vst)));
        engine.lock().unwrap().start();

        // {
        //     // Example: Sound from a file:
        //     // let file = std::fs::File::open("output.wav").unwrap();
        //     // let beep_sink = stream_handle.play_once(BufReader::new(file)).unwrap();
        //     // beep_sink.set_volume(0.3);
        //
        //     // Example: Sound from a generative source:
        //     // stream_handle.play_raw(
        //     //     SineWave::new(1000.0)
        //     //         .take_duration(Duration::from_secs(30))
        //     //         .amplify(0.1))
        //     //     .unwrap();
        //
        //     // Sound from the VST host:
        //     stream_handle.play_raw(OutputSource::new(&vst, &buffer_size)).unwrap();
        // }

        // {
        //     println!("Processing events.");
        //     engine.process(&make_a_note());
        // }

        // {
        //     let input = MidiInput::new("midir").unwrap();
        //     let mut port_idx = None;
        //     println!("Midi input ports:");
        //     let ports = input.ports();
        //     for (i, port) in ports.iter().enumerate() {
        //         let name = input.port_name(&port).unwrap();
        //         println!("\t{}", name);
        //
        //         // if name.starts_with("Digital Piano") {
        //         if name.starts_with("MPK mini 3") {
        //             port_idx = Some(i);
        //             println!("Selected MIDI input: '{}'", name);
        //             break;
        //         }
        //     }
        //
        //     let port = ports.get(port_idx
        //         .expect("No midi input selected."))
        //         .unwrap();
        //     let seq_engine = engine.clone();
        //     let conn = input.connect(
        //         &port,
        //         "midi-input",
        //         move|t, ev, _data| {
        //             println!("MIDI event: {} {:?} {}", t, ev, ev.len());
        //             if ev[0] == 254 { return; }
        //             let mut ev_buf = [0u8; 3];
        //             for (i, x) in ev.iter().enumerate() {
        //                 ev_buf[i] = *x;
        //             }
        //             let note = Event::Midi(MidiEvent {
        //                 data: ev_buf,
        //                 delta_frames: 0,
        //                 live: true,
        //                 note_length: None,
        //                 note_offset: None,
        //                 detune: 0,
        //                 note_off_velocity: 0,
        //             });
        //             seq_engine.lock().unwrap().process(note);
        //
        //             // Trying to estimate rodio delays in the playback.
        //             // Playing a simple source directly to exclude potential delays in the VST.
        //             // stream_handle.play_raw(
        //             //     SineWave::new(1000.0)
        //             //         .take_duration(Duration::from_millis(100)))
        //             //     .unwrap()
        //         },
        //         (),
        //     ).unwrap();
        //
        //     let mut input = String::new();
        //     input.clear();
        //     stdin().read_line(&mut input).unwrap(); // wait for next enter key press
        //     conn.close();
        // }

        { // Play MIDI from SMD
            let data = std::fs::read("yellow.mid").unwrap();
            // Parse the raw bytes
            let smf = midly::Smf::parse(&data).unwrap();
            // Use the information
            println!("midi file has {} tracks, format is {:?}.", smf.tracks.len(), smf.header.format);
            let track = smf.tracks.get(0).unwrap();

            println!("SMF header {:#?}", &smf.header);
            assert!(&smf.header.format == &Format::SingleTrack,
                    "MIDI SMF format is not supported {:#?}", &smf.header.format);
            // TODO Also should support Tempo messages. Tempo messages set micros per beat.
            // Default tempo is 120 beats per minute and default signature 4/4
            let tick_per_beat = beat_duration(&smf.header.timing);
            let beat_per_sec = 120 / 60; // Default is 120 beats/minute.
            let usec_per_tick = 1_000_000 / (beat_per_sec * tick_per_beat);
            println!("t/b {:#?}, b/s  {:#?}, usec/tick {:#?}",
                     &tick_per_beat, &beat_per_sec, &usec_per_tick);

            println!("First event of the 1st track is {:#?}", &track[..10]);

            // Try doing some modifications
            let mut i = 0;
            while i < track.len() {
                let event = track[i.to_owned()];
                println!("Event: {:#?}", &event);

                sleep(Duration::from_micros(event.delta.as_int() as u64 * usec_per_tick as u64));

                match event.kind {
                    TrackEventKind::Midi { channel: _, message } => {
                        engine.lock().unwrap().process(make_a_note(&message));
                        println!("  PLAYED");
                    }
                    _ => ()
                };
                i += 1;
            }

            // smf.save("rewritten.mid").unwrap();
        }


        // {
        //     // Example: output to a file:
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
        //  `   }
        //     let wav_data = wav::BitDepth::ThirtyTwoFloat(pcm_data.to_owned());
        //     wav::write(wav_header, &wav_data, &mut out_file).unwrap();
        //     drop(out_file);
        // }
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

fn beat_duration(timing: &Timing) -> u32 {
    match timing {
        Timing::Metrical(d) => d.as_int() as u32,
        _ => panic!("Timing format {:#?} is not supported.", timing)
    }
}

fn make_a_note<'a>(message: &midly::MidiMessage) -> Event<'a> {
    let note_event = midly::live::LiveEvent::Midi {
        channel: 1.into(),
        message: message.to_owned(),
    };
    let mut track_event_buf = [0u8; 3];
    let mut cursor = midly::io::Cursor::new(&mut track_event_buf);
    note_event.write(&mut cursor).unwrap();
    println!("Event bytes {:?}\n", track_event_buf);
    Event::Midi(MidiEvent {
        data: track_event_buf,
        delta_frames: 0,
        live: true,
        note_length: None,
        note_offset: None,
        detune: 0,
        note_off_velocity: 0,
    })
}

/*
  Using https://github.com/iced-rs/iced/blob/0.3/examples/counter/src/main.rs
  as a stub implementation for starters.
*/

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

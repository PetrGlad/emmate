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
use cpal::{BufferSize, ChannelCount, SampleFormat, SampleRate, SupportedBufferSize, SupportedStreamConfig};
use cpal::SupportedBufferSize::Range;
use iced::{
    Alignment, button, Button, Column, Element, Sandbox, Settings, Text,
};
use midir::MidiInput;
use midly::{TrackEvent, TrackEventKind};
use midly::io::Cursor;
use midly::MidiMessage::NoteOn;
use midly::TrackEventKind::Midi;
use rodio::{cpal, OutputStream, Source};
use rodio::source::SineWave;
use rodio::source::TakeDuration;
use vst::api::Events;
use vst::event::Event;
use vst::host::{Host, HostBuffer};
use vst::plugin::Plugin;
use wav::BitDepth;
use crate::midi_vst::{OutputSource, Vst};
use vst::event::{MidiEvent};

pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }

    let audio_host = cpal::default_host();
    let out_device = lookup_device("pipewire").unwrap();
    // .unwrap_or(audio_host.default_output_device().unwrap());
    println!("Default output device: {:?}", out_device.name());
    let out_conf = out_device.default_output_config().unwrap();
    println!("Default output config: {:?}", out_conf);
    let out_conf = SupportedStreamConfig::new(
        out_conf.channels(),
        out_conf.sample_rate(),
        // Trying to reduce delays. There seem no good correlation between the buffer size and the midi->sound lag.
        SupportedBufferSize::Range { min: 64, max: 128 },
        out_conf.sample_format(),
    );
    println!("Output config: {:?}", out_conf);
    let (_stream, stream_handle) =
        rodio::OutputStream::try_from_device_config(&out_device, out_conf.to_owned()).unwrap();
    // let (_stream, stream_handle) = rodio::OutputStream::try_default().unwrap();

    {
        let vst = Vst::init(&out_conf.sample_rate());
        {
            // Example: Sound from a file:
            // let file = std::fs::File::open("output.wav").unwrap();
            // let beep_sink = stream_handle.play_once(BufReader::new(file)).unwrap();
            // beep_sink.set_volume(0.3);

            // Example: Sound from a generative source:
            // stream_handle.play_raw(
            //     SineWave::new(1000.0)
            //         .take_duration(Duration::from_millis(200)))
            //     .unwrap();

            // Sound from the VST host:
            stream_handle.play_raw(OutputSource::new(&vst, 1)).unwrap();
        }

        // {
        //     let events_list = [make_a_note()];
        //     let mut events_buffer = vst::buffer::SendEventBuffer::new(events_list.len());
        //     events_buffer.store_events(events_list);
        //     // XXX See https://github.com/RustAudio/vst-rs/pull/160.
        //     // Do not know yet how to make a plugin reference available in a Host implementation.
        //     // events_buffer.send_events(events_list, &mut *(VST.host.lock().unwrap()));
        //     let plugin_holder = vst.plugin.clone();
        //     println!("Processing events.");
        //     let mut plugin = plugin_holder.lock().unwrap();
        //     plugin.process_events(events_buffer.events());
        // }

        {
            let input = MidiInput::new("midir").unwrap();
            let mut port_idx = None;
            println!("Midi input ports:");
            let ports = input.ports();
            for (i, port) in ports.iter().enumerate() {
                let name = input.port_name(&port).unwrap();
                println!("\t{}", name);

                // if name.starts_with("Digital Piano") {
                if name.starts_with("MPK mini 3") {
                    port_idx = Some(i);
                    println!("Selected MIDI input: '{}'", name);
                    break;
                }
            }

            let port = ports.get(port_idx
                .expect("No midi input selected."))
                .unwrap();
            let plugin_holder2 = vst.plugin.clone();
            let conn = input.connect(
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
                    let events_list = [note];
                    let mut events_buffer = vst::buffer::SendEventBuffer::new(events_list.len());
                    events_buffer.store_events(events_list);
                    let mut plugin = plugin_holder2.lock().unwrap();
                    plugin.process_events(events_buffer.events());

                    // Trying to estimate rodio delays in the playback.
                    // Playing a simple source directly to exclude potential delays in the VST.
                    // stream_handle.play_raw(
                    //     SineWave::new(1000.0)
                    //         .take_duration(Duration::from_millis(100)))
                    //     .unwrap()
                },
                (),
            ).unwrap();

            let mut input = String::new();
            input.clear();
            stdin().read_line(&mut input).unwrap(); // wait for next enter key press
            conn.close();
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
        //     }
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

fn make_a_note<'a>() -> Event<'a> {
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
    note
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

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn lookup_device(device_selector: &str)
                 -> Option<cpal::Device> {
    // Copy-paste from cpal examples
    println!("Supported hosts:\n  {:?}", cpal::ALL_HOSTS);
    let available_hosts = cpal::available_hosts();
    println!("Available hosts:\n  {:?}", available_hosts);

    for host_id in available_hosts {
        println!("{}", host_id.name());
        let host = cpal::host_from_id(host_id).unwrap();

        let default_in = host.default_input_device().map(|e| e.name().unwrap());
        let default_out = host.default_output_device().map(|e| e.name().unwrap());
        println!("  Default Input Device:\n    {:?}", default_in);
        println!("  Default Output Device:\n    {:?}", default_out);

        let devices = host.devices().unwrap();
        println!("  Devices: ");
        for (device_index, device) in devices.enumerate() {
            println!("  {}. \"{}\"", device_index + 1, device.name().unwrap());
            if device.name().unwrap().starts_with(device_selector) {
                return Some(device);
            }

            // Input configs
            if let Ok(conf) = device.default_input_config() {
                println!("    Default input stream config:\n      {:?}", conf);
            }
            let input_configs = match device.supported_input_configs() {
                Ok(f) => f.collect(),
                Err(e) => {
                    println!("    Error getting supported input configs: {:?}", e);
                    Vec::new()
                }
            };
            // Too verbose
            // if !input_configs.is_empty() {
            //     println!("    All supported input stream configs:");
            //     for (config_index, config) in input_configs.into_iter().enumerate() {
            //         println!(
            //             "      {}.{}. {:?}",
            //             device_index + 1,
            //             config_index + 1,
            //             config
            //         );
            //     }
            // }

            // Output configs
            if let Ok(conf) = device.default_output_config() {
                println!("    Default output stream config:\n      {:?}", conf);
            }
            let output_configs = match device.supported_output_configs() {
                Ok(f) => f.collect(),
                Err(e) => {
                    println!("    Error getting supported output configs: {:?}", e);
                    Vec::new()
                }
            };
            // Too verbose
            // if !output_configs.is_empty() {
            //     println!("    All supported output stream configs:");
            //     for (config_index, config) in output_configs.into_iter().enumerate() {
            //         println!(
            //             "      {}.{}. {:?}",
            //             device_index + 1,
            //             config_index + 1,
            //             config
            //         );
            //     }
            // }
        }
    }

    None
}
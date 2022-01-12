/*
  Using https://github.com/iced-rs/iced/blob/0.3/examples/counter/src/main.rs
  as a stub implementation for starters.
*/
use std::io::BufWriter;
use std::path::Path;
use std::{error, result};
use iced::{
    button, Align, Button, Column, Element, Sandbox, Settings, Text,
};
use midly::{TrackEvent, TrackEventKind};
use vst::host::{Host, HostBuffer, PluginLoader};
use vst::plugin::{CanDo, Plugin};
use std::sync::{Arc, Mutex};
use midly::io::Cursor;
use midly::MidiMessage::NoteOn;
use midly::TrackEventKind::Midi;
use vst::api::{Events, Supported};
use vst::event::{Event, MidiEvent};

#[allow(dead_code)]
struct VstHost;

impl Host for VstHost {
    fn automate(&self, index: i32, value: f32) {
        println!("Parameter {} had its value changed to {}", index, value);
    }
}

pub fn main() {
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

    { // Load VST example
        let host = Arc::new(Mutex::new(VstHost));

        // let path = Path::new("/home/petr/opt/Pianoteq 7/x86-64bit/Pianoteq 7.lv2/Pianoteq_7.so");
        let path = Path::new("/home/petr/opt/Pianoteq 7/x86-64bit/Pianoteq 7.so");
        println!("Loading {}...", path.to_str().unwrap());

        // Load the plugin
        let mut loader =
            PluginLoader::load(path, Arc::clone(&host)).unwrap_or_else(|e| panic!("Failed to load plugin: {}", e));

        // Create an instance of the plugin
        let mut instance = loader.instance().unwrap();

        // Get the plugin information
        let info = instance.get_info();
        println!(
            "Loaded '{}':\n\t\
             Vendor: {}\n\t\
             Presets: {}\n\t\
             Parameters: {}\n\t\
             VST ID: {}\n\t\
             Version: {}\n\t\
             Initial Delay: {} samples\n\t\
             Inputs {}\n\t\
             Outputs {}",
            info.name, info.vendor, info.presets, info.parameters, info.unique_id,
            info.version, info.initial_delay, info.inputs, info.outputs
        );

        // Initialize the instance
        instance.init();
        println!("Initialized VST instance.");
        println!("Can receive MIDI events {}", instance.can_do(CanDo::ReceiveMidiEvent) == Supported::Yes);

        // TODO Instantiate vst::api::Event

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

        let input_count = info.inputs as usize;
        let output_count = info.outputs as usize;
        let mut host_buffer: HostBuffer<f32> = HostBuffer::new(input_count, output_count);
        let buf_size = 1 << 14;
        let inputs = vec![vec![0.0; buf_size]; input_count];
        let mut outputs = vec![vec![0.0; buf_size]; output_count];
        let mut audio_buffer = host_buffer.bind(&inputs, &mut outputs);

        instance.suspend(); // Can only set these parameters in suspended state.
        instance.set_sample_rate(48000f32);
        // instance.set_block_size(128);

        instance.resume();
        instance.start_process();

        let mut events_buffer = vst::buffer::SendEventBuffer::new(1);
        events_buffer.send_events_to_plugin([note], &mut instance);
        instance.process(&mut audio_buffer);

        for out in &outputs {
            println!("Output {:?}\n", out);
        }
        // TODO Output the sound to default audio device

        {
            // I want to hear it
            use std::fs::File;
            use std::path::Path;

            // let mut inp_file = File::open(Path::new("data/sine.wav"))?;
            // let (header, data) = wav::read(&mut inp_file)?;
            let wav_header = wav::Header::new(
                wav::WAV_FORMAT_IEEE_FLOAT, 1, 48000, 32);

            let wav_data = wav::BitDepth::ThirtyTwoFloat(outputs[0].to_owned());
            let mut out_file = File::create(Path::new("output.wav")).unwrap();
            wav::write(wav_header, &wav_data, &mut out_file).unwrap();
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
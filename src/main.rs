use std::sync::{Arc, Mutex};
use std::time::Duration;
use cpal::{BufferSize, StreamConfig};
use cpal::SampleFormat::F32;
use cpal::traits::{DeviceTrait, HostTrait};
use iced::{Alignment, Application, Command, Element, executor, Length, Settings, Theme,
           widget::Button, widget::Column, widget::Text};
use iced::widget::{container, Row, Space};
use midir::{MidiInput, MidiInputConnection};
use rodio::{cpal, OutputStream};
use vst::event::Event;
use vst::event::MidiEvent;

use crate::engine::Engine;
use crate::midi::SmfSource;
use crate::midi_vst::{OutputSource, Vst};
use crate::stave::{events_to_notes, Stave};
use crate::track::{Track, TrackTime};

mod midi_vst;
mod midi;
mod engine;
mod events;
mod stave;
mod track;

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

    // GUI
    Ed::run(Settings {
        id: Option::None,
        window: iced::window::Settings::default(),
        flags: Init {},
        default_font: Option::None,
        default_text_size: 20.0,
        text_multithreading: false,
        antialiasing: true,
        exit_on_close_request: true,
        try_opengles_first: false,
    }).unwrap()
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
    let engine = Engine::new(vst);
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

#[derive(Default)]
struct Ed {
    stave: Stave,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    ZoomIn(TrackTime),
    ZoomOut(TrackTime),
    Stave,
}

pub struct Init {
    // TODO Pass reference to the engine as init
}

impl Application for Ed {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = Init;

    fn new(_init: Init) -> (Self, Command<Message>) {
        // TODO ???????????????????? Do not want to setup engine here. How to start it not using statics?
        // Load some sample data to show on stave. Should be read from same source as the engine's.
        let smf_data = std::fs::read("yellow.mid").unwrap();
        let events = midi::load_smf(&smf_data);
        let notes = events_to_notes(events.0);
        (
            Ed { stave: Stave { track: Track { notes }, time_scale: 12e-8f32 } },
            Command::none()
        )
    }

    fn title(&self) -> String {
        String::from("emmate")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::ZoomIn(_at) => {
                self.stave.time_scale *= 1.05;
            }
            Message::ZoomOut(_at) => {
                self.stave.time_scale *= 0.95;
            }
            Message::Stave => ()
        };
        Command::none()
    }

    fn view(&self) -> Element<Message> {
        Column::new()
            .padding(20)
            .spacing(20)
            .push(container(Text::new("I cannot do this, Petr."))
                .width(Length::Fill))
            .push(self.stave.view().map(move |_message| Message::Stave))
            .push(
                Row::new()
                    .align_items(Alignment::Start)
                    .push(
                        Button::new(Text::new("Zoom in"))
                            .on_press(Message::ZoomIn(Duration::from_micros(0))),
                    )
                    .push(Space::new(10, 0))
                    .push(
                        Button::new(Text::new("Zoom out"))
                            .on_press(Message::ZoomOut(Duration::from_micros(0))),
                    ))
            .into()
    }
}

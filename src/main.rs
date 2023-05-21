use cpal::traits::{DeviceTrait, HostTrait};
use cpal::SampleFormat::F32;
use cpal::{BufferSize, StreamConfig};
use iced::keyboard::Event::KeyPressed;
use iced::keyboard::{KeyCode};
use iced::widget::{container, Row, Space};
use iced::{
    executor, widget::Button, widget::Column, widget::Text, Alignment, Application, Command,
    Element, Length, Settings, Theme,
};
use iced_native::Event::Keyboard;
use iced_native::Subscription;
use midir::{MidiInput, MidiInputConnection};
use rodio::{cpal, OutputStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vst::event::Event;
use vst::event::MidiEvent;

use crate::engine::Engine;
use crate::midi::SmfSource;
use crate::midi_vst::{OutputSource, Vst};
use crate::stave::{events_to_notes, Stave};
use crate::track::{Track, TrackSource, TrackTime};

mod engine;
mod events;
mod midi;
mod midi_vst;
mod stave;
mod track;

pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }
    // Stream reference keeps it open.
    let (_stream, mut engine) = setup_audio_engine();

    if false {
        // Want the section to compile still for now
        // Play MIDI from an SMD file.
        let smf_data = std::fs::read("yellow.mid").unwrap();
        let smf_midi_source = SmfSource::new(smf_data);
        engine.lock().unwrap().add(Box::new(smf_midi_source));
    }

    // This source does not support the damper controller yet.
    let smf_data = std::fs::read("yellow.mid").unwrap();
    let events = midi::load_smf(&smf_data);
    let track = Arc::new(Box::new(Track {
        notes: events_to_notes(events.0, events.1 as u64),
    }));
    {
        let track_midi_source = TrackSource::new(track.clone());
        engine.lock().unwrap().add(Box::new(track_midi_source));
    }

    let mut midi_inputs = vec![]; // Keeps inputs open
    midi_inputs.push(midi_keyboard_input("Digital Piano", &mut engine));
    midi_inputs.push(midi_keyboard_input("XPIANOGT", &mut engine));
    midi_inputs.push(midi_keyboard_input("MPK mini 3", &mut engine));

    // GUI
    Ed::run(Settings {
        id: None,
        window: iced::window::Settings::default(),
        flags: UiInit {
            engine: engine.clone(),
            track: track.clone(),
        },
        default_font: Option::None,
        default_text_size: 20.0,
        text_multithreading: false,
        antialiasing: true,
        exit_on_close_request: true,
        try_opengles_first: false,
    })
    .unwrap()
}

fn setup_audio_engine() -> (OutputStream, Arc<Mutex<Engine>>) {
    let buffer_size = 256;
    let audio_host = cpal::default_host();
    let out_device = audio_host.default_output_device().unwrap();
    println!("Default output device: {:?}", out_device.name());
    let out_conf = out_device.default_output_config().unwrap();
    println!("Default output config: {:?}", out_conf);
    assert_eq!(out_conf.sample_format(), F32);
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
    stream_handle
        .play_raw(OutputSource::new(&vst, &buffer_size))
        .unwrap();
    let engine = Engine::new(vst);
    (stream, engine.start())
}

fn midi_keyboard_input(
    name_prefix: &str,
    engine: &mut Arc<Mutex<Engine>>,
) -> Option<MidiInputConnection<()>> {
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
    let port = ports.get(port_idx.unwrap()).unwrap();
    let seq_engine = engine.clone();
    Some(
        input
            .connect(
                &port,
                "midi-input",
                move |t, ev, _data| {
                    println!("MIDI event: {} {:?} {}", t, ev, ev.len());
                    if ev[0] == 254 {
                        return;
                    }
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
            )
            .unwrap(),
    )
}

struct Ed {
    engine: Arc<Mutex<Engine>>,
    stave: Stave,
}

#[derive(Debug, Clone)]
pub enum Message {
    ZoomIn(TrackTime),
    ZoomOut(TrackTime),
    Event(iced_native::Event),
    Stave,
}

pub struct UiInit {
    engine: Arc<Mutex<Engine>>,
    track: Arc<Box<Track>>,
}

impl Application for Ed {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = UiInit;

    fn new(init: UiInit) -> (Self, Command<Message>) {
        (
            Ed {
                engine: init.engine,
                stave: Stave {
                    track: init.track,
                    time_scale: 5e-9f32,
                },
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("emmate")
    }

    fn subscription(&self) -> Subscription<Message> {
        iced_native::subscription::events().map(Message::Event)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::ZoomIn(_at) => {
                self.stave.time_scale *= 1.05;
            }
            Message::ZoomOut(_at) => {
                self.stave.time_scale *= 0.95;
            }
            Message::Event(Keyboard(KeyPressed {
                key_code: KeyCode::Space,
                modifiers,
            })) if modifiers.is_empty() => self.engine.lock().unwrap().toggle_pause(),
            Message::Event(ev) => println!("System event {:?}", ev),
            Message::Stave => (),
        };
        Command::none()
    }

    fn view(&self) -> Element<Message> {
        Column::new()
            .padding(20)
            .spacing(20)
            .push(container(Text::new("I cannot do this, Petr.")).width(Length::Fill))
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
                    ),
            )
            .into()
    }
}

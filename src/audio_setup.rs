use std::sync::mpsc::Sender;
use std::sync::{mpsc, Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::SampleFormat::F32;
use cpal::{BufferSize, StreamConfig};
use midir::{MidiInput, MidiInputConnection};
use midly::live::LiveEvent;
use rodio::OutputStream;
use vst::event::{Event, MidiEvent};

use crate::engine::{Engine, EngineCommand};
use crate::midi_vst::{OutputSource, Vst};

pub fn setup_audio_engine(
    vst_plugin_path: &String,
    vst_preset_id: &i32,
) -> (OutputStream, Arc<Mutex<Engine>>, Sender<Box<EngineCommand>>) {
    let buffer_size = 256;
    let audio_host = cpal::default_host();
    let out_device = audio_host.default_output_device().unwrap();
    println!("INFO Default output device: {:?}", out_device.name());
    let out_conf = out_device.default_output_config().unwrap();
    println!("INFO Default output config: {:?}", out_conf);
    assert_eq!(out_conf.sample_format(), F32); // Required by VST
    let out_stream_conf = StreamConfig {
        channels: out_conf.channels(),
        sample_rate: out_conf.sample_rate(),
        buffer_size: BufferSize::Fixed(buffer_size),
    };
    println!("INFO Output config: {:?}", out_stream_conf);
    let (stream, stream_handle) = rodio::OutputStream::try_from_config(
        &out_device,
        &out_stream_conf,
        &out_conf.sample_format(),
    )
    .unwrap();
    let (command_sender, command_receiver) = mpsc::channel();
    let vst = Vst::init(
        vst_plugin_path,
        &out_stream_conf.sample_rate,
        &buffer_size,
        *vst_preset_id,
    );
    stream_handle
        .play_raw(OutputSource::new(&vst, &buffer_size))
        .unwrap();
    let engine = Engine::new(vst, command_receiver);
    (stream, engine.start(), command_sender)
}

// TODO (refactoring) Convert this into event source? Note: on pause engine stops all sources,
//      may want this to be active when not playing the track (e.g. to make edits audible).
pub fn midi_keyboard_input(
    name_prefix: &str,
    engine: &mut Arc<Mutex<Engine>>,
) -> Option<MidiInputConnection<()>> {
    let input = MidiInput::new("emmate").unwrap();
    let mut port_idx = None;
    println!("Available MIDI input ports:");
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
    let engine = engine.clone();
    // TODO Probably we should have an input source for this case. It may need
    //      special handling while the engine is paused.
    Some(
        input
            .connect(
                &port,
                "midi-input",
                move |t, ev, _data| {
                    {
                        let le = LiveEvent::parse(ev)
                            .expect("Unparseable input controller event.")
                            .to_static();
                        println!("Input MIDI event: {} {:?}", t, le);
                    }
                    if ev[0] == 254 {
                        return; // Ignore keep-alives.
                    }
                    let mut ev_buf = [0u8; 3];
                    for (i, x) in ev.iter().enumerate() {
                        ev_buf[i] = *x;
                    }
                    let event = Event::Midi(MidiEvent {
                        data: ev_buf,
                        delta_frames: 0,
                        live: true,
                        note_length: None,
                        note_offset: None,
                        detune: 0,
                        note_off_velocity: 0,
                    });
                    // TODO (bug) Sustain events seem to be ignored by the VST plugin.
                    engine.lock().unwrap().process(event);
                },
                (),
            )
            .unwrap(),
    )
}

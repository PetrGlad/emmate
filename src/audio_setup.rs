use rodio::OutputStream;
use std::sync::{Arc, Mutex};
use cpal::SampleFormat::F32;
use cpal::{BufferSize, StreamConfig};
use midir::{MidiInput, MidiInputConnection};
use midly::live::LiveEvent;
use vst::event::{Event, MidiEvent};
use cpal::traits::{DeviceTrait, HostTrait};
use crate::engine::Engine;
use crate::midi_vst::{OutputSource, Vst};

pub fn setup_audio_engine() -> (OutputStream, Arc<Mutex<Engine>>) {
    let buffer_size = 256;
    let audio_host = cpal::default_host();
    let out_device = audio_host.default_output_device().unwrap();
    println!("INFO Default output device: {:?}", out_device.name());
    let out_conf = out_device.default_output_config().unwrap();
    println!("INFO Default output config: {:?}", out_conf);
    assert_eq!(out_conf.sample_format(), F32);
    let sample_format = F32; // To use with vst.
    let out_conf = StreamConfig {
        channels: out_conf.channels(),
        sample_rate: out_conf.sample_rate(),
        buffer_size: BufferSize::Fixed(buffer_size),
    };
    println!("INFO Output config: {:?}", out_conf);
    let (stream, stream_handle) =
        rodio::OutputStream::try_from_config(&out_device, &out_conf, &sample_format).unwrap();
    let vst = Vst::init(&out_conf.sample_rate, &buffer_size);
    stream_handle
        .play_raw(OutputSource::new(&vst, &buffer_size))
        .unwrap();
    // let (sender, receiver) = iced_native::futures::channel::mpsc::channel(100);
    let engine = Engine::new(vst /*, sender*/);
    (stream, engine.start())
}

pub fn midi_keyboard_input(
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
                    {
                        ////// DEBUG
                        let x = ev.clone();
                        let le = LiveEvent::parse(x)
                            .expect("Unparsable input controller event.")
                            .to_static();
                        println!("MIDI event parsed: {} {:?}", t, le);
                        // MIDI event parsed: 22643573 Ok(Midi { channel: u4(0), message: Controller { controller: u7(66), value: u7(0) } })
                    }
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

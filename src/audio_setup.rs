use std::sync::mpsc::Sender;
use std::sync::{mpsc, Arc, Mutex};

use midir::{MidiInput, MidiInputConnection, MidiOutputConnection};
use midly::live::LiveEvent;

use crate::engine::{Engine, EngineCommand};

pub fn setup_audio_engine(
    midi_output: MidiOutputConnection,
) -> (Arc<Mutex<Engine>>, Sender<Box<EngineCommand>>) {
    let (command_sender, command_receiver) = mpsc::channel();
    let engine = Engine::new(midi_output, command_sender.clone(), command_receiver);
    (engine.start(), command_sender)
}

// TODO (refactoring) Convert this into event source? Note: on pause engine stops all sources,
//      may want this to be active when not playing the track (e.g. to make edits audible).
pub fn midi_keyboard_input(
    name_prefix: &str,
    engine: &mut Arc<Mutex<Engine>>,
) -> Option<MidiInputConnection<()>> {
    let input = MidiInput::new("emmate").unwrap();
    let mut port_idx = None;
    log::debug!("Available MIDI input ports:");
    let ports = input.ports();
    for (i, port) in ports.iter().enumerate() {
        let name = input.port_name(&port).unwrap();
        log::debug!("\t{}", name);
        if name.starts_with(name_prefix) {
            port_idx = Some(i);
            log::info!("Selected MIDI input: '{}'", name);
            break;
        }
    }
    if port_idx == None {
        log::warn!("WARN No midi input selected.");
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
                    let le = LiveEvent::parse(ev)
                        .expect("Unparseable input controller event.")
                        .to_static();
                    println!("Input MIDI event: {} {:?}", t, le);
                    if ev[0] == 254 {
                        return; // Ignore keep-alives.
                    }
                    // TODO (bug) Effect of sustain events does not last for some reason.
                    //      Triggering noise is there but subsequent notes do not feel the effect.
                    engine.lock().unwrap().process(le);
                },
                (),
            )
            .expect("MIDI input port"),
    )
}

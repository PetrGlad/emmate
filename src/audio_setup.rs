use midir::{MidiInput, MidiInputConnection, MidiOutputConnection};
use midly::live::LiveEvent;

use crate::engine::{Engine, EngineHandle};

pub fn setup_audio_engine(midi_output: MidiOutputConnection) -> EngineHandle {
    Engine::new(midi_output).start()
}

// TODO (refactoring) Convert this into event source? Note: on pause engine stops all sources,
//      may want this to be active when not playing the track (e.g. to make edits audible).
pub fn midi_keyboard_input(
    name_prefix: &str,
    engine: &EngineHandle,
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
        log::warn!("No midi input port for \"{}*\".", name_prefix);
        return None;
    }
    let port = ports.get(port_idx.unwrap()).unwrap();
    let engine = engine.clone();
    // TODO Probably I should have an input source for this case. It may need
    //      special handling while the engine is paused. Some events still have
    //      to be played when transport paused for audition/pass-through.
    Some(
        input
            .connect(
                &port,
                "midi-input",
                move |t, ev, _data| {
                    if ev.first() == Some(&254) {
                        return; // Ignore keep-alives.
                    }
                    let Ok(le) = LiveEvent::parse(ev).map(|le| le.to_static()) else {
                        log::warn!("Unparseable input controller event {:?}.", ev);
                        return;
                    };
                    log::info!("Input MIDI event: {} {:?}.", t, le);
                    // TODO (bug) Effect of sustain events does not last for some reason.
                    //      Triggering noise is there but subsequent notes do not feel the effect.
                    let _ = engine.send(Box::new(move |engine| engine.process(le)));
                },
                (),
            )
            .expect("MIDI input port"),
    )
}

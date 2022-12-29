use midly::{Format, Smf, Timing, TrackEventKind};
use crate::engine::{EngineEvent, MidiSource};
use vst::api::Events;
use vst::event::{Event, MidiEvent};

pub struct SmfSource {
    smf: Smf<'static>,
    usec_per_tick: u32,
    i: usize,
}

impl SmfSource {
    pub fn new(smf: Smf<'static>) -> SmfSource {
        println!("SMF header {:#?}", &smf.header);
        println!("SMF file has {} tracks, format is {:?}.", smf.tracks.len(), smf.header.format);
        assert!(&smf.header.format == &Format::SingleTrack,
                "MIDI SMF format is not supported {:#?}", &smf.header.format);
        assert!(smf.tracks.len() > 0, "No tracks in SMF file. At least one is required");
        // println!("First event of the 1st track is {:#?}", &track[..10]);
        let usec_per_tick = usec_per_midi_tick(&smf.header.timing);
        SmfSource { smf, usec_per_tick, i: 0 }
    }
}

fn usec_per_midi_tick(timing: &Timing) -> u32 {
    let tick_per_beat = beat_duration(timing);
    let beat_per_sec = 120 / 60; // Default is 120 beats/minute.
    let usec_per_tick = 1_000_000 / (beat_per_sec * tick_per_beat);
    println!("t/b {:#?}, b/s  {:#?}, usec/tick {:#?}",
             &tick_per_beat, &beat_per_sec, &usec_per_tick);
    usec_per_tick
}

fn beat_duration(timing: &Timing) -> u32 {
    // TODO Also should support Tempo messages. Tempo messages set micros per beat.
    // Default tempo is 120 beats per minute and default signature 4/4
    match timing {
        Timing::Metrical(d) => d.as_int() as u32,
        _ => panic!("Timing format {:#?} is not supported.", timing)
    }
}

impl MidiSource for SmfSource {}

impl Iterator for SmfSource {
    type Item = EngineEvent;

    fn next(&mut self) -> Option<Self::Item> {
        let track = &self.smf.tracks[0];
        while self.i < track.len() {
            let event = track[self.i.to_owned()];
            //  println!("Event: {:#?}", &event);
            match event.kind {
                TrackEventKind::Midi { channel: _, message } => {
                    return Some(EngineEvent {
                        dt: event.delta.as_int() as u32 * self.usec_per_tick,
                        midi_event: make_a_note(&message),
                    });
                }
                _ => ()
            };
            self.i += 1;
        }
        None
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


// { // Use ALSA to read midi events
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


// { // MIDI load/modify example
//         let data = std::fs::read("yellow.mid").unwrap();
//         // Parse the raw bytes
//         let mut smf = midly::Smf::parse(&data).unwrap();
//         // Use the information
//         println!("midi file has {} tracks, format is {:?}.", smf.tracks.len(), smf.header.format);
//         let track = smf.tracks.get_mut(0).unwrap();
//
//         println!("The 1st track is {:#?}", &track);
//
//         // Try doing some modifications
//         let mut i = 0;
//         while i < track.len() {
//             let skip = match track[i].kind {
//                 TrackEventKind::Meta(_) => true,
//                 _ => false
//             };
//             if skip {
//                 track.remove(i);
//             } else {
//                 i += 1;
//             }
//         }
//
//         smf.save("rewritten.mid").unwrap();
//     }
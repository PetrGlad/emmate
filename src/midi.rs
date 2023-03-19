use std::ops::{Add, Deref};
use std::sync::Arc;
use std::time::{Duration, Instant};
use midly::{Format, Smf, Timing, TrackEvent, TrackEventKind};
use crate::engine::{Engine, EngineEvent, EventSource, TransportTime};
use vst::api::Events;
use vst::event::{Event, MidiEvent};

pub struct SmfSource {
    events: Vec<TrackEvent<'static>>,
    tick: Duration,
    current_idx: usize,
    running_at: TransportTime,
}

impl SmfSource {
    pub fn new(smf_data: Vec<u8>) -> SmfSource {
        let smf_data = smf_data.to_owned();
        let smf = midly::Smf::parse(&smf_data).unwrap();
        println!("SMF header {:#?}", &smf.header);
        println!("SMF file has {} tracks, format is {:?}.", smf.tracks.len(), smf.header.format);
        assert!(&smf.header.format == &Format::SingleTrack,
                "MIDI SMF format is not supported {:#?}", &smf.header.format);
        assert!(smf.tracks.len() > 0, "No tracks in SMF file. At least one is required.");
        // println!("First event of the 1st track is {:#?}", &track[..10]);
        let usec_per_tick = usec_per_midi_tick(&smf.header.timing);
        let mut events = vec![];
        for me in &smf.tracks[0] {
            let event = me.to_static();
            events.push(event);
        }
        SmfSource {
            events,
            tick: Duration::from_micros(usec_per_tick as u64),
            current_idx: 0,
            running_at: 0,
        }
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

impl EventSource for SmfSource {
    fn is_running(&self) -> bool {
        self.current_idx < self.events.len()
    }

    fn reset(&mut self, at: &TransportTime) {
        self.running_at = at.to_owned();
    }

    fn next(&mut self, at: &TransportTime) -> Option<EngineEvent> {
        let track = &self.events;
        while self.is_running() {
            let event = track[self.current_idx];
            let running_at = self.running_at.to_owned() + self.tick.as_micros() as u64 * event.delta.as_int() as u64;
            if running_at > *at {
                return None;
            }
            self.running_at = running_at;
            self.current_idx += 1;
            if let Some(lev) = event.kind.as_live_event() {
                return Some(EngineEvent { event: lev });
            }
        }
        None
    }
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
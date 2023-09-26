use std::collections::BinaryHeap;
use std::time::Duration;

use midly::{Format, Header, MidiMessage, Smf, Timing, Track, TrackEvent};
use midly::io::WriteResult;
use midly::live::LiveEvent;
use midly::MidiMessage::Controller;
use midly::num::u15;

use crate::engine::{EngineEvent, EventSource, TransportTime};
use crate::track::{ChannelId, ControllerId, Level, Pitch};

pub struct SmfSource {
    events: Vec<TrackEvent<'static>>,
    tick: Duration,
    current_idx: usize,
    running_at: TransportTime,
}

pub fn load_smf(smf_data: &Vec<u8>) -> (Vec<TrackEvent<'static>>, u32) {
    let smf = Smf::parse(smf_data).unwrap();
    println!("SMF header {:#?}", &smf.header);
    println!(
        "SMF file has {} tracks, format is {:?}.",
        smf.tracks.len(),
        smf.header.format
    );
    assert_eq!(
        &smf.header.format,
        &Format::SingleTrack,
        "MIDI SMF format {:#?} is not supported.",
        &smf.header.format
    );
    assert!(
        smf.tracks.len() > 0,
        "No tracks in SMF file. At least one is required."
    );
    // println!("Starting events of the 1st track are {:#?}", &track[..10]);
    let usec_per_tick = usec_per_tick(&smf.header.timing);
    let mut events = vec![];
    for me in &smf.tracks[0] {
        let event = me.to_static();
        events.push(event);
    }
    (events, usec_per_tick)
}

pub fn serialize_smf(
    events: Vec<TrackEvent<'static>>,
    usec_per_tick: u32,
    out: &mut Vec<u8>,
) -> WriteResult<Vec<u8>> {
    let mut track = Track::new();
    track.extend_from_slice(events.as_slice());
    let timing = timing_from_usec_per_tick(usec_per_tick);
    let header = Header::new(Format::SingleTrack, timing);
    dbg!(header);
    let mut smf = Smf::new(header);
    smf.tracks.push(track);
    smf.write(out)
}

impl SmfSource {
    pub fn new(smf_data: Vec<u8>) -> SmfSource {
        let (events, usec_per_tick) = load_smf(&smf_data);
        SmfSource {
            events,
            tick: Duration::from_micros(usec_per_tick as u64),
            current_idx: 0,
            running_at: 0,
        }
    }
}

// Default SMF tempo is 120 beats per minute and default signature 4/4
const DEFAULT_BEATS_PER_SEC: u32 = 120 / 60;

fn usec_per_tick(timing: &Timing) -> u32 {
    let tick_per_beat = ticks_per_beat(timing);
    let usec_per_tick = 1_000_000 / (DEFAULT_BEATS_PER_SEC * tick_per_beat);
    println!(
        "tick/beat {:#?}, beat/second  {:#?}, usec/tick {:#?}",
        tick_per_beat, DEFAULT_BEATS_PER_SEC, usec_per_tick
    );
    usec_per_tick
}

fn ticks_per_beat(timing: &Timing) -> u32 {
    // TODO Also maybe support Tempo messages. Tempo messages set micros per beat.
    match timing {
        Timing::Metrical(d) => d.as_int() as u32,
        _ => panic!("Timing format {:#?} is not supported.", timing),
    }
}

fn timing_from_usec_per_tick(usec_per_tick: u32) -> Timing {
    Timing::Metrical(u15::from(
        (1_000_000f32 / (usec_per_tick as f32 * DEFAULT_BEATS_PER_SEC as f32)) as u16,
    ))
}

impl EventSource for SmfSource {
    fn is_running(&self) -> bool {
        self.current_idx < self.events.len()
    }

    fn seek(&mut self, at: &TransportTime) {
        assert!(
            self.running_at > *at,
            "SmfSource back reset is not supported."
        );
        self.running_at = at.to_owned();
    }

    fn next(&mut self, at: &TransportTime, queue: &mut BinaryHeap<EngineEvent>) {
        let track = &self.events;
        while self.is_running() {
            let event = track[self.current_idx];
            let running_at =
                self.running_at + self.tick.as_micros() as u64 * event.delta.as_int() as u64;
            if running_at > *at {
                return;
            }
            self.running_at = running_at;
            self.current_idx += 1;
            if let Some(lev) = event.kind.as_live_event() {
                queue.push(EngineEvent {
                    at: *at,
                    event: lev,
                });
            }
        }
    }
}

pub fn note_on(channel: ChannelId, pitch: Pitch, velocity: Level) -> LiveEvent<'static> {
    LiveEvent::Midi {
        channel: channel.into(),
        message: MidiMessage::NoteOn {
            key: pitch.into(),
            vel: velocity.into(),
        },
    }
}

pub fn note_off(channel: ChannelId, pitch: Pitch, velocity: Level) -> LiveEvent<'static> {
    LiveEvent::Midi {
        channel: channel.into(),
        message: MidiMessage::NoteOff {
            key: pitch.into(),
            // Not sure if this actually affects anything.
            vel: velocity.into(),
        },
    }
}

pub fn controller_set(
    channel: ChannelId,
    controller_id: ControllerId,
    value: Level,
) -> LiveEvent<'static> {
    LiveEvent::Midi {
        channel: channel.into(),
        message: Controller {
            controller: controller_id.into(),
            value: value.into(),
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_conversion() {
        let timing = Timing::Metrical(u15::from(1000u16));
        assert_eq!(usec_per_tick(&timing), 500);
        let timing = Timing::Metrical(u15::from(19200u16));
        assert_eq!(usec_per_tick(&timing), 26);

        let timing = Timing::Metrical(u15::from(1234u16));
        assert_eq!(timing_from_usec_per_tick(usec_per_tick(&timing)), timing);
    }
}

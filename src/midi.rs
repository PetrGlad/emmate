// { // Use ALSA to read midi events
//
//     // TODO: Replace ALSA with midir for reading events. See https://docs.rs/midir/0.7.0/midir/struct.MidiInput.html
//
//     // Diagnostics commands
//     //   amidi --list-devices
//     //   aseqdump --list
//     //   aseqdump --port='24:0'
//
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
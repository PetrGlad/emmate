use std::any::Any;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::{Arg, Command};
use eframe::egui::TextBuffer;
use midir::MidiInput;
use midly::live::LiveEvent;
use midly::num::u28;
use midly::{Format, Header, Smf, Timing, Track, TrackEvent, TrackEventKind};
use signal_hook::consts::signal::*;
use signal_hook::flag;

const DEFAULT_USEC_PER_TICK: u32 = 500; // 120 BPM with 1000 ticks per beat
const DEFAULT_TICKS_PER_BEAT: u16 = 1000;

struct RecordingSession {
    first_event_time: Option<Instant>,
    last_event_time: Option<Instant>,
    usec_per_tick: u32,
    events: Vec<TrackEvent<'static>>,
}

impl RecordingSession {
    fn new() -> Self {
        RecordingSession {
            first_event_time: None,
            last_event_time: None,
            usec_per_tick: DEFAULT_USEC_PER_TICK,
            events: Vec::new(),
        }
    }

    fn add_event(&mut self, event: LiveEvent<'static>) {
        let now = Instant::now();
        if self.first_event_time.is_none() {
            self.first_event_time = Some(now);
        }
        let elapsed_since_last = self
            .last_event_time
            .map(|t| now.duration_since(t))
            .unwrap_or(Duration::ZERO);
        let delta_ticks =
            (elapsed_since_last.as_micros() as u64 / self.usec_per_tick as u64) as u32;
        self.last_event_time = Some(now);

        // Convert LiveEvent to TrackEventKind
        if let Some(kind) = Self::live_event_to_track_event_kind(event) {
            self.events.push(TrackEvent {
                delta: u28::from(delta_ticks),
                kind,
            });
        }
    }

    fn live_event_to_track_event_kind(
        event: LiveEvent<'static>,
    ) -> Option<TrackEventKind<'static>> {
        match event {
            LiveEvent::Midi { channel, message } => Some(TrackEventKind::Midi { channel, message }),
            LiveEvent::Common(_) => None, // Skip common events for now
            LiveEvent::Realtime(_) => None, // Skip realtime events
        }
    }

    fn save_to_file(&mut self, path: &PathBuf) -> std::io::Result<()> {
        if self.first_event_time.is_none() {
            assert!(self.events.is_empty());
        }
        assert!(!self.events.is_empty() && self.last_event_time.is_some());
        let mut file_path = path.clone();
        file_path.set_file_name(format!(
            "{}-{}-{}ev-{}s",
            &path.file_name().unwrap().to_string_lossy(),
            chrono::Local::now().format("%Y%m%dT%H%M%S"),
            self.events.len(),
            self.last_event_time
                .unwrap()
                .duration_since(self.first_event_time.unwrap())
                .as_secs()
        ));

        self.events.push(TrackEvent {
            delta: u28::from(0),
            kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
        });

        let timing = Timing::Metrical(midly::num::u15::from(DEFAULT_TICKS_PER_BEAT));
        let header = Header::new(Format::SingleTrack, timing);
        let mut smf = Smf::new(header);

        let mut track = Track::new();
        track.extend_from_slice(&self.events);
        smf.tracks.push(track);

        let mut output = Vec::new();
        smf.write(&mut output).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("MIDI write error: {:?}", e),
            )
        })?;

        println!("Writing recording to {:}", &file_path.display());
        let mut file = File::create(&file_path)?;
        file.write_all(&output)?;
        println!("Wrote {} events to {:?}", self.events.len(), &file_path);
        self.events.clear();

        Ok(())
    }
}

fn list_midi_inputs() -> Result<(), Box<dyn std::error::Error>> {
    let midi_input = MidiInput::new("midi-recorder-list")?;
    let ports = midi_input.ports();

    if ports.is_empty() {
        println!("No MIDI input ports available.");
    } else {
        println!("Available MIDI input ports:");
        for (i, port) in ports.iter().enumerate() {
            let name = midi_input.port_name(port)?;
            println!("  [{}] {}", i, name);
        }
    }
    Ok(())
}

fn start_recording(
    port_name_prefix: &str,
    output_path: PathBuf,
    duration_secs: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let midi_input = MidiInput::new("midi-recorder")?;
    let ports = midi_input.ports();

    // Find port matching the prefix
    let mut selected_port = None;
    for port in &ports {
        let name = midi_input.port_name(port)?;
        if name.starts_with(port_name_prefix) {
            println!("Selected MIDI input: '{}'", name);
            selected_port = Some(port);
            break;
        }
    }

    let port = selected_port
        .ok_or_else(|| format!("No MIDI input port found matching '{}'", port_name_prefix))?;

    let session = Arc::new(Mutex::new(RecordingSession::new()));
    let session_clone = session.clone();

    println!("Recording MIDI...");
    if let Some(duration) = duration_secs {
        println!("Will record for {} seconds.", duration);
    } else {
        println!("Press Ctrl+C to stop recording.");
    }
    println!();

    let _connection = midi_input.connect(
        port,
        "midi-recorder-input",
        move |timestamp, message, _| {
            // Skip active sensing and clock messages
            if message[0] == 0xFE || message[0] == 0xF8 {
                return;
            }

            if let Ok(live_event) = LiveEvent::parse(message) {
                let static_event = live_event.to_static();
                println!("@ {}: {:?}", timestamp, static_event);

                let mut session = session_clone.lock().unwrap();
                session.add_event(static_event);
            }
        },
        (),
    )?;

    let stop = Arc::new(AtomicBool::new(false));
    flag::register(SIGINT, Arc::clone(&stop))?;

    if let Some(duration) = duration_secs {
        std::thread::sleep(Duration::from_secs(duration));
    } else {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    // Save the recording

    let mut session = session.lock().unwrap();
    session.save_to_file(&output_path)?;

    println!("Bye.");
    Ok(())
}

fn main() {
    let matches = Command::new("midi-recorder")
        .version("0.1.0")
        .author("Emmate contributors")
        .about("Records MIDI events from a controller to a MIDI file")
        .arg(
            Arg::new("list")
                .short('l')
                .long("list")
                .help("List available MIDI input ports")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("port")
                .short('p')
                .long("port")
                .value_name("PORT_PREFIX")
                .help("MIDI input port name prefix to use")
                .required_unless_present("list"),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("FILE")
                .help("Output MIDI file path")
                .value_parser(clap::value_parser!(PathBuf))
                .required_unless_present("list"),
        )
        .arg(
            Arg::new("duration")
                .short('d')
                .long("duration")
                .value_name("SECONDS")
                .help("Recording duration in seconds (optional, Ctrl+C to stop manually)")
                .value_parser(clap::value_parser!(u64)),
        )
        .get_matches();

    let result = if matches.get_flag("list") {
        list_midi_inputs()
    } else {
        let port_prefix = matches.get_one::<String>("port").unwrap();
        let output_path = matches.get_one::<PathBuf>("output").unwrap().clone();
        let duration = matches.get_one::<u64>("duration").copied();

        start_recording(port_prefix, output_path, duration)
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

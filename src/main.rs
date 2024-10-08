use eframe::{egui, Theme};
use midir::os::unix::VirtualOutput;
use midir::MidiOutput;

use crate::app::EmApp;
use crate::config::Config;
use crate::midi::SmfSource;
use crate::project::Project;
use crate::track_source::TrackSource;

mod app;
mod audio_setup;
mod changeset;
mod common;
mod config;
mod engine;
mod midi;
mod project;
mod stave;
mod track;
mod track_edit;
mod track_history;
mod track_source;
mod util;

pub type Pix = f32;

pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }
    let arg_matches = clap::Command::new("emmate")
        .version("0.3.1")
        .author("Petr <petrglad@gmail.com>")
        .about("MIDI editor")
        .arg(
            clap::arg!(--"config-file" <VALUE>)
                .value_parser(clap::value_parser!(std::path::PathBuf)),
        )
        .arg(
            clap::arg!(--"midi-file" <VALUE>)
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .default_value("yellow.mid"),
        )
        .get_matches();
    // No configurable values at the moment, keeping it here to keep config loader compilable.
    let _config = Config::load(arg_matches.get_one::<std::path::PathBuf>("config-file"));

    let midi_file_path = arg_matches
        .get_one::<std::path::PathBuf>("midi-file")
        .unwrap();
    println!("MIDI file name {:?}", midi_file_path);
    let project = Project::open_file(midi_file_path);

    let midi_output = MidiOutput::new("emmate")
        .expect("MIDI sequencer client")
        .create_virtual("emmate")
        .expect("MIDI sequencer out");

    // Stream and engine references keep them open.
    let (mut engine, engine_command_sender) = audio_setup::setup_audio_engine(midi_output);
    if false {
        // Want the section to still be compilable.
        // Play MIDI from an SMD file.
        let smf_data = std::fs::read(midi_file_path).unwrap();
        let smf_midi_source = SmfSource::new(smf_data);
        engine_command_sender
            .send(Box::new(|engine| engine.add(Box::new(smf_midi_source))))
            .unwrap();
    }

    {
        let track_midi_source = TrackSource::new(project.history.borrow().track.clone());
        engine_command_sender
            .send(Box::new(|engine| engine.add(Box::new(track_midi_source))))
            .unwrap();
    }

    let mut midi_inputs = vec![]; // Keeps inputs open
    midi_inputs.push(audio_setup::midi_keyboard_input(
        "Digital Piano",
        &mut engine,
    ));
    midi_inputs.push(audio_setup::midi_keyboard_input("XPIANOGT", &mut engine));
    midi_inputs.push(audio_setup::midi_keyboard_input("MPK mini 3", &mut engine));

    // GUI
    let native_options = eframe::NativeOptions {
        default_theme: Theme::Light,
        min_window_size: Some(egui::emath::Vec2 { x: 300.0, y: 200.0 }),
        ..Default::default()
    };
    eframe::run_native(
        "emmate",
        native_options,
        Box::new(|ctx| Box::new(EmApp::new(ctx, engine_command_sender, project))),
    )
    .expect("Emmate UI")
}

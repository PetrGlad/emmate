use std::sync::{Arc, RwLock};

use eframe::{egui, Theme};

use crate::app::EmApp;
use crate::config::Config;
use crate::midi::SmfSource;
use crate::project::Project;
use crate::track::Track;
use crate::track_source::TrackSource;

mod app;
mod audio_setup;
mod common;
mod config;
mod engine;
mod events;
mod midi;
mod midi_vst;
mod project;
mod stave;
mod track;
mod track_history;
mod track_source;
mod util;

pub type Pix = f32;

pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }
    let arg_matches = clap::Command::new("MyApp")
        .version("0.2")
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
    let config = Config::load(arg_matches.get_one::<std::path::PathBuf>("config-file"));

    let midi_file_path = arg_matches
        .get_one::<std::path::PathBuf>("midi-file")
        .unwrap();
    println!("MIDI file name {:?}", midi_file_path);
    let history = Project::open_file(midi_file_path).history;

    // Stream and engine references keep them open.
    let (_stream, mut engine, engine_command_sender) =
        audio_setup::setup_audio_engine(&config.vst_plugin_path, &config.vst_preset_id);
    if false {
        // Want the section to still be compilable.
        // Play MIDI from an SMD file.
        let smf_data = std::fs::read(midi_file_path).unwrap();
        let smf_midi_source = SmfSource::new(smf_data);
        engine_command_sender
            .send(Box::new(|engine| engine.add(Box::new(smf_midi_source))))
            .unwrap();
    }

    let mut track = Track::default();
    track.load_from(&history.current_snapshot_path());
    let track = Arc::new(RwLock::new(track));
    {
        let track_midi_source = TrackSource::new(track.clone());
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
    let ui_track = track.clone();
    eframe::run_native(
        "emmate",
        native_options,
        Box::new(|ctx| Box::new(EmApp::new(ctx, engine_command_sender, ui_track, history))),
    )
    .expect("Emmate UI")
}

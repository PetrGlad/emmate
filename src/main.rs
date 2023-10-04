use std::sync::{Arc, RwLock};

use clap::{Arg, ArgAction};
use eframe::{egui, Theme};

use track::to_lane_events;

use crate::app::EmApp;
use crate::config::Config;
use crate::midi::SmfSource;
use crate::track::Lane;
use crate::track_source::TrackSource;

mod app;
mod audio_setup;
mod config;
mod engine;
mod events;
mod midi;
mod midi_vst;
mod project;
mod stave;
mod track;
mod track_source;

pub type Pix = f32;

pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }
    let cmd = clap::Command::new("emmate")
        .arg(
            Arg::new("config-file")
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("input-file")
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .action(ArgAction::Set)
                .default_value("yellow.mid"),
        );
    let matches = cmd.get_matches();
    let config = Config::load(matches.get_one::<std::path::PathBuf>("config-file"));

    let midi_file_path = matches.get_one::<std::path::PathBuf>("input-file").unwrap();
    println!("MIDI file name {:?}", midi_file_path);
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
    // let smf_data = std::fs::read("yellow.mid").unwrap();
    let smf_data = std::fs::read(midi_file_path).unwrap();
    let events = midi::load_smf(&smf_data);
    let track = Arc::new(RwLock::new(Lane::new(to_lane_events(
        events.0,
        events.1 as u64,
    ))));
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
        Box::new(|ctx| Box::new(EmApp::new(ctx, engine_command_sender, ui_track))),
    )
    .expect("Emmate UI")
}

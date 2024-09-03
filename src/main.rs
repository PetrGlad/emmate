use crate::app::EmApp;
use crate::config::Config;
use crate::midi::SmfSource;
use crate::project::Project;
use crate::track_source::TrackSource;
use clap::Command;
use clap_complete::aot as ccomplete;
use eframe::{egui, Theme, WindowBuilder, WindowBuilderHook};
use midir::os::unix::VirtualOutput;
use midir::MidiOutput;
use std::io;

mod app;
mod audio_setup;
mod changeset;
mod common;
mod config;
mod engine;
mod ev;
mod midi;
mod project;
mod stave;
mod track;
mod track_edit;
mod track_history;
mod track_source;
mod util;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub type Pix = f32;

pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }
    let arg_matches = build_cli().get_matches();
    if let Some(generator) = arg_matches
        .get_one::<ccomplete::Shell>("shell-completion-script")
        .copied()
    {
        eprintln!("Generating shell completion file for {generator}...");
        let mut cli = build_cli();
        let command_name = cli.get_name().to_string();
        ccomplete::generate(generator, &mut cli, command_name, &mut io::stdout());
        return;
    }

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
    midi_inputs.push(audio_setup::midi_keyboard_input("MPK mini 3", &mut engine));

    // GUI
    let window_builder = Box::new(|wb: egui::ViewportBuilder| {
        let mut wb = wb.clone();
        wb.min_inner_size
            .replace(egui::emath::Vec2 { x: 250.0, y: 150.0 });
        wb
    });
    let native_options = eframe::NativeOptions {
        default_theme: Theme::Light,
        window_builder: Some(window_builder),
        ..Default::default()
    };
    eframe::run_native(
        "emmate",
        native_options,
        Box::new(|ctx| Ok(Box::new(EmApp::new(ctx, engine_command_sender, project)))),
    )
    .expect("Emmate UI")
}

fn build_cli() -> Command {
    let mut cli = clap::command!()
        // let arg_matches = clap::Command::new("emmate")
        //     .version(VERSION)
        //     .author("Petr <petrglad@gmail.com>")
        //     .about("MIDI editor")
        .arg(
            clap::arg!(--"config-file" <FILE>)
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .value_hint(clap::ValueHint::FilePath),
        )
        .arg(
            clap::arg!(--"midi-file" <FILE>)
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .default_value("yellow.mid")
                .value_hint(clap::ValueHint::FilePath),
        )
        .arg(
            clap::arg!(--"shell-completion-script" <SHELL_NAME>)
                .value_parser(clap::value_parser!(ccomplete::Shell)),
        );
    cli
}

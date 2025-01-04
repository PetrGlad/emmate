use clap::Command;
use clap_complete::aot as ccomplete;
use eframe::egui;
use indoc::indoc;
use midir::os::unix::VirtualOutput;
use midir::MidiOutput;
use std::io;
use std::path::PathBuf;
use std::process::exit;
use std::sync::mpsc::Sender;

use crate::app::EmApp;
use crate::config::Config;
use crate::engine::EngineCommand;
use crate::midi::SmfSource;
use crate::project::Project;
use crate::track_source::TrackSource;

mod app;
mod audio_setup;
mod changeset;
mod clipboard;
mod common;
mod config;
mod engine;
mod midi;
mod project;
mod range;
mod stave;
mod track;
mod track_edit;
mod track_history;
mod track_source;
mod util;

pub type Pix = f32;

pub fn main() {
    let arg_matches = build_cli().get_matches();
    if arg_matches.get_flag("log") {
        env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .filter_module("emmate", log::LevelFilter::Trace)
            .init();
    } else {
        env_logger::init();
    }

    log::info!(
        "Starting {} version {}.",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    if let Some(generator) = arg_matches
        .get_one::<ccomplete::Shell>("shell-completion-script")
        .copied()
    {
        log::info!("Generating shell completion file for {generator}...");
        let mut cli = build_cli();
        let command_name = cli.get_name().to_string();
        ccomplete::generate(generator, &mut cli, command_name, &mut io::stdout());
        return;
    }

    // No configurable values at the moment, keeping it here to keep config loader compilable.
    let _config = Config::load(arg_matches.get_one::<PathBuf>("config-file"));

    let project_dir;

    // TODO (improvement) Use positional argument instead  and auto-detect how to open the path (midi or project).
    if let Some(midi_file_path) = arg_matches.get_one::<PathBuf>("midi-file") {
        log::info!("Opening MIDI file {:?}", midi_file_path);
        project_dir = Project::init_from_midi_file(midi_file_path);
    } else if let Some(path) = arg_matches.get_one::<PathBuf>("project") {
        project_dir = path.clone();
        log::info!("Opening project {:?}", &project_dir);
    } else {
        log::error!("No MIDI file or project dir to opne is given.");
        exit(1);
    }
    let project = Project::open(&project_dir);

    let midi_output = MidiOutput::new(common::APP_NAME)
        .expect("MIDI sequencer client")
        .create_virtual(common::APP_NAME)
        .expect("MIDI sequencer out");

    // Stream and engine references keep them open.
    let (mut engine, engine_command_sender) = audio_setup::setup_audio_engine(midi_output);

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
    let window_builder = Box::new(|wb: egui::ViewportBuilder| {
        let mut wb = wb.clone();
        wb.min_inner_size
            .replace(egui::emath::Vec2 { x: 250.0, y: 150.0 });
        wb
    });
    let native_options = eframe::NativeOptions {
        window_builder: Some(window_builder),
        ..Default::default()
    };
    eframe::run_native(
        common::APP_NAME,
        native_options,
        Box::new(|ctx| {
            ctx.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(EmApp::new(ctx, engine_command_sender, project)))
        }),
    )
    .expect("Emmate UI")
}

// Play MIDI from an SMD file.
fn play_midi_file(midi_file_path: &PathBuf, engine_command_sender: &Sender<Box<EngineCommand>>) {
    let smf_data = std::fs::read(midi_file_path).unwrap();
    let smf_midi_source = SmfSource::new(smf_data);
    engine_command_sender
        .send(Box::new(|engine| engine.add(Box::new(smf_midi_source))))
        .unwrap();
}

fn build_cli() -> Command {
    clap::command!()
        .arg(
            clap::arg!(--"config-file" <FILE>)
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .value_hint(clap::ValueHint::FilePath),
        )
        .arg(
            clap::arg!(--"midi-file" <FILE>)
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .value_hint(clap::ValueHint::FilePath),
        )
        .arg(
            clap::arg!(--"project" <FILE>)
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .value_hint(clap::ValueHint::FilePath),
        )
        .arg(
            clap::arg!(--"shell-completion-script" <SHELL_NAME>)
                .value_parser(clap::value_parser!(ccomplete::Shell)),
        )
        .arg(
            clap::arg!(--"log")
                .value_parser(clap::value_parser!(bool))
                .help("Enable detailed log. RUST_LOG environment variable is also supported."),
        )
        .help_template(indoc! {
        "{name}{tab}{about}
            Version{tab}{version}
            Authors{tab}{author}

            {usage-heading}
            {tab}{usage}

            {all-args}
            "})
}

use clap::Command;
use clap_complete::aot as ccomplete;
use eframe::egui;
use indoc::indoc;
use midir::os::unix::VirtualOutput;
use midir::MidiOutput;
use std::io;
use std::path::PathBuf;
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
    let _config = Config::load(arg_matches.get_one::<std::path::PathBuf>("config-file"));

    let midi_file_path = arg_matches
        .get_one::<std::path::PathBuf>("midi-file")
        .unwrap_or_else(|| {
            log::error!("Missing argument 'midi-file'");
            std::process::exit(1);
        });
    log::info!("MIDI file name {:?}", midi_file_path);
    let project = Project::open_file(midi_file_path);

    let midi_output = MidiOutput::new("emmate")
        .expect("MIDI sequencer client")
        .create_virtual("emmate")
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
        "emmate",
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

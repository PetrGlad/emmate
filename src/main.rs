use std::sync::{Arc, Mutex};

use eframe::{egui, Theme};
use midir::{MidiInputConnection};
use rodio::{cpal, OutputStream};

use crate::app::EmApp;
use crate::engine::{Engine, StatusEvent};
use crate::midi::SmfSource;
use crate::midi_vst::{Vst};
use crate::stave::{Stave, to_lane_events};
use crate::track::{Lane, TrackSource, TrackTime};

mod app;
mod audio_setup;
mod engine;
mod events;
mod midi;
mod midi_vst;
mod stave;
mod track;

pub fn main() {
    {
        // use log::*;
        // stderrlog::new()/*.module(module_path!())*/.verbosity(Level::Trace).init().unwrap();
    }
    // Stream reference keeps it open.
    let (_stream, mut engine) = audio_setup::setup_audio_engine();
    if false {
        // Want the section to still be compilable for now
        // Play MIDI from an SMD file.
        let smf_data = std::fs::read("yellow.mid").unwrap();
        let smf_midi_source = SmfSource::new(smf_data);
        engine.lock().unwrap().add(Box::new(smf_midi_source));
    }
    let smf_data = std::fs::read("yellow.mid").unwrap();
    let events = midi::load_smf(&smf_data);
    let track = Arc::new(Box::new(Lane {
        events: to_lane_events(events.0, events.1 as u64),
    }));
    {
        let track_midi_source = TrackSource::new(track.clone());
        engine.lock().unwrap().add(Box::new(track_midi_source));
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
    let ui_engine = engine.clone();
    eframe::run_native(
        "emmate",
        native_options,
        Box::new(|_cc| Box::new(EmApp::new(ui_engine, ui_track))),
    )
    .expect("Emmate UI")
}

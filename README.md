# emmate

Off grid MIDI editor with following goals:

* Not a DAW (would like it to be but do not have enough time for that).
* Do not care (much) about measures. Primarily aimed at piano real recordings without strict tempo/bars.
* A feature absent in _any_ other midi editor I could get my hands on (both commercial and free ones): removing a note
  can shift the tail of the track left to fill the gap. In some editors this is actually doable but cumbersome at best.
* Playing/editing very long (over several thousands events) files.
  Those files are usually recordings of real performance (e.g. from MIDI keyboard).
* Comfortable workflow with keyboard as primary input.
* Allows making fine adjustments of notes and tempo.
* Unlimited undo/redo. Never loose session data. Non destructive edits, do not override original files.
* Blackbox recording (aways-on MIDI recording).

I'd love to see this in one of commercial or open-source DAWs and even pay money for that, but that does not seem to
ever happen.

## Status

Not even a prototype, still figuring things out. I am learning both Rust, MIDI and sound processing at once so the code
should not be expected to be a good style example.

## Build

In case you get "No package 'freetype2' found" on Linux
`apt install libxft-dev`.

ALSA wrapper dependency
`apt install libasound2-dev`.

As an example synth plugin you can use `amsynth`.
I use Pianoteq, but that is a commercial product.

## TODO

Prototype checklist
- [ ] Time or note selection in UI.
- [ ] Configuration file (VST plugin path and MIDI input configuration).
- [ ] Transport controls (play/pause, rewind, step, pause).
- [x] Support sustain pedal (as a note?).
- [x] Play time cursor.
- [x] Scale stave time (horizontally) with mouse wheel.
- [x] Share project's note data between stave pane and engine.
- [x] See how to use a hot key for an action.
- [x] A UI window with simple text message.
- [x] Load/decode a MIDI file.
- [x] Load a MIDI->PCM plugin (https://github.com/RustAudio/vst-rs/blob/master/examples/simple_host.rs).
- [x] Play MIDI.
- [x] Show MIDI pane.
- [x] Piano roll.
- [x] Paint notes.

Have to explore following options for the further development

* Use [Tokio](https://github.com/tokio-rs/tokio) for scheduling instead of spinning in a thread.
* Or, be based on [Dropseed](https://github.com/MeadowlarkDAW/dropseed) (which is used in MeadowlarkDAW)
* Or, ideally, participate in [MeadowlarkDAW](https://github.com/MeadowlarkDAW/Meadowlark) - but I am not comfortable to
  take on that yet.
* Think if using midi events directly makes sense. See e.g. `track::to_lane_events`. 
  This will require
    * To handle ignored/unused events along with notes and sustain.
    * Midi events have starting times relative to previous ones. May need some indexing mechanism (e.g. range tree) that
      would help to find absolute timings of the midi events.

# Implementation notes

Unless stated/declared otherwise

* Integer-typed times are in microseconds.
* Ranges assumed to be half open (excluding end/highest value).

# Notes

SMD - Standard MIDI File.

Rodio and other libraries use interleaved stream format 1 sample of each channel, then 2 sample of each channel and so
on (ch1s1, ch2s1, ...., ch1s2, ch2s2, ....).

Diagnostic commands

* `amidi --list-devices`
* `aseqdump --list`
* `aseqdump --port='24:0'`

MIDI CC controllers list

* https://nickfever.com/music/midi-cc-list
* https://soundslikejoe.com/2014/03/midi-cc-reference-chart/
* https://www.whippedcreamsounds.com/midi-cc-list/
* https://anotherproducer.com/online-tools-for-musicians/midi-cc-list/

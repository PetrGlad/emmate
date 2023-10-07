# emmate

Off grid MIDI editor with following goals:

* Not a DAW (would like it to be but do not have enough time for that).
* Do not care (much) about measures. Primarily aimed at piano real recordings without strict tempo/bars.
* A feature absent in _any_ other midi editor I could get my hands on (both commercial and free ones): removing a note
  can shift the tail of the track left to fill the gap. In some editors this is actually doable but cumbersome at best.
* Playing/editing very long (up to about 20K of events) files.
  Those files are usually recordings of real performances (e.g. from MIDI keyboard).
* Comfortable workflow with keyboard.
* Allows making fine adjustments of notes and tempo.
* Unlimited undo/redo. Never loose session data. Non destructive edits, do not override original files.
* Blackbox recording (always-on MIDI recording).

I'd love to see this in one of commercial or open-source DAWs and even pay money for that, but that does not seem to
ever happen.

## Status

Prototype, still figuring things out. I am learning Rust, MIDI, sound processing, and UI library all at once
so the code is not an example of a good style.

## Build

In case you get "No package 'freetype2' found" on Linux
`apt install libxft-dev`.

ALSA wrapper dependency
`apt install libasound2-dev`.

As an example synth plugin you can use `amsynth`.
I use Pianoteq, but that is a commercial product.

## TODO

- [ ] Note input (with mouse).
- [ ] Note selection.
- [ ] Allow editing sustain events.
- [ ] Do not save new version when there are no changes.
- [ ] Multi-track UI (for snippets and copy/paste buffer).
- [ ] Time marks on stave.
- [ ] Time bookmarks.
- [ ] Find a way to separate actions from view logic with egui. It looks too messy now.
- [x] Simple undo/redo.
- [x] Time selection.
- [x] Configuration file (VST plugin path and MIDI input configuration).
- [x] Transport controls (play/pause, rewind, pause).
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

Big scary problems

* Should eventually migrate away from VST2 (unsupported and unreliable). VST3 is GPL. LV2 seem like a decent choice.
  Here seem to be a LV2 host implementation in Rust https://github.com/wmedrano/livi-rs. Can also implement one from
  scratch, or use JACK API and register emmate as a MIDI sequencer. Pipewire seem to SUPPORT JACK also (see `pw-jack`).
* May need to use midi events directly (instead of intermediate internal representation). E.g.
  `track::to_lane_events` may not be necessary. This will require
    * To handle ignored/unused events along with notes and sustain.
    * Midi events have starting times relative to previous ones. May need some indexing mechanism (e.g. a range tree)
      that would help to find absolute timings of the midi events, and connect beginnings and endings of notes.

Have to explore following options for the further development

* Use [Tokio](https://github.com/tokio-rs/tokio) for scheduling instead of spinning in a thread.
* Ideally, this should be a part of some open source DAW. I found one that is written in Rust,
  [MeadowlarkDAW](https://github.com/MeadowlarkDAW/Meadowlark). It is open source but not a collaborative project (as
  stated in its README).

# Implementation notes

Unless stated otherwise

* Integer-typed times are in microseconds.
* Ranges assumed to be half open, excluding end (upper bound) value.

# Notes

SMD - Standard MIDI File.

Rodio and other libraries use interleaved stream format 1 sample of each channel, then 2 sample of each channel and so
on (ch1s1, ch2s1, ...., ch1s2, ch2s2, ....). This seems to be a convention but is not documented anywhere for some
reason.

Diagnostic commands

* `amidi --list-devices`
* `aseqdump --list`
* `aseqdump --port='24:0'`

MIDI CC controllers list

* https://nickfever.com/music/midi-cc-list
* https://soundslikejoe.com/2014/03/midi-cc-reference-chart/
* https://www.whippedcreamsounds.com/midi-cc-list/
* https://anotherproducer.com/online-tools-for-musicians/midi-cc-list/

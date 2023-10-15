# emmate

Off grid MIDI editor with following goals:

* Not a DAW (would like it to be but do not have enough time for that).
* Do not care (much) about measures. Primarily aimed at piano real recordings without strict tempo/bars.
* A feature absent in other midi editors I could get my hands on (both commercial and free ones): removing a piece
  of MIDI recording as one can remove a time fragment from PCM recording. For some reason DAW authors insist on handling
  MIDI recordings differently from PCM sound recordings. In some editors this is actually doable but cumbersome at best.
* Playing/editing very long (up to about 20K of events) files.
  Those files are usually recordings of real performances (e.g. from MIDI keyboard).
* Comfortable workflow with keyboard.
* Allows making fine adjustments of notes and tempo.
* Unlimited undo/redo. Never loose session data. Non destructive edits, do not override original files.
* Blackbox recording (always-on MIDI recording).

I'd love to see this in one of commercial or open-source DAWs and even pay money for that, but I do not see it
happening.

## Status

Prototype, still figuring things out. I am learning Rust, MIDI, sound processing, and UI library all at once
so the code is not an example of a good style.

## Build

In case you get "No package 'freetype2' found" on Linux
`apt install libxft-dev`.

ALSA wrapper dependency
`apt install libasound2-dev`.

As an VST synth plugin you can use `amsynth`, for example.
I use Pianoteq, but that is a commercial product.

## TODO

- [ ] Editing sustain events.
- [ ] Note input (with mouse).
- [ ] Automatically create an undo snapshot on every edit command.
- [ ] Do not save a new snapshot when there are no changes.
- [ ] Time marks on stave (minute:second from the beginning).
- [ ] Consider TransportTime to be signed (see also StaveTime). There are too many conversions forth and back. Times can
  be restricted to positives only in the engine.
- [ ] Have a separate edit-position and play-start cursors, so it is easier to jump back and listen to the modified
  version.
- [ ] Time bookmarks.
- [ ] Find a way to separate actions from view logic with egui. It looks too messy now.
- [ ] Minimize use of unwrap. The biggest contention currently is event data shared between engine and stave.
- [ ] Multi-track UI (for snippets, flight recorder, and copy/paste buffer). Can show only one at a time, though. Use
  tabs?
- [ ] Zoom to fit whole composition.
- [ ] Visual hint for out-of-view selected notes. Scroll to the earliest of the selected notes on an action, if none of
  them are currently visible.
- [ ] Flight recorder (always record what is coming from the MIDI controller into a separate file).
- [ ] Copy/cut/paste.
- [x] Note selection.
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

* Should eventually migrate away from VST2. The bindings are unsupported anymore, and there are SIGSEGVs that I have not
  managed to resolve so far. Also, there are licensing issues with the VST2 API itself.
  VST3 is GPL. LV2 seem like a decent choice. Here seem to be an LV2 host implementation in
  Rust https://github.com/wmedrano/livi-rs. Can also implement one from scratch, or use JACK API and register emmate as
  a MIDI sequencer. Pipewire seems to SUPPORT JACK API as well (see `pw-jack`).
* May need to use midi events directly (instead of intermediate internal representation). E.g. `track::to_lane_events`
  may not be necessary. In particular tail shifts will become simpler. This will require
    * To handle ignored/unused events along with notes and sustain.
    * Midi events have starting times relative to previous ones. May need some indexing mechanism (e.g. a range tree)
      that would help to find absolute timings of the midi events, and connect beginnings and endings of notes. MIDI
      allows overlapping notes index should be able to handle that.
* Optimize rendering drawing only visible items, may also need some index. In the simplest casevisible notes can be
  determined when zoom changes, and then re-use the visible set.

Have to explore following options for the further development

* Use [Tokio](https://github.com/tokio-rs/tokio) for scheduling instead of explicitly spinning in a thread.
* Ideally, the editor should be a part of some open source DAW. I found one that is written in
  Rust, [MeadowlarkDAW](https://github.com/MeadowlarkDAW/Meadowlark). It is open source but not a collaborative
  project (as stated in its README).

# Implementation notes

Unless stated otherwise

* Integer-typed times are in microseconds.
* Ranges assumed to be half open, excluding end (upper bound) value.

# Notes

SMD - Standard MIDI File.

Rodio and other libraries use interleaved stream format: 1st sample of each channel, then 2nd sample of each channel,
and so on (ch1s1, ch2s1, ...., ch1s2, ch2s2, ....). This seems to be a convention but is not documented anywhere for
some reason.

Diagnostic commands

* `amidi --list-devices`
* `aseqdump --list`
* `aseqdump --port='24:0'`

MIDI CC controller ids list

* https://nickfever.com/music/midi-cc-list
* https://soundslikejoe.com/2014/03/midi-cc-reference-chart/
* https://www.whippedcreamsounds.com/midi-cc-list/
* https://anotherproducer.com/online-tools-for-musicians/midi-cc-list/

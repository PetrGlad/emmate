# emmate

Grid-less MIDI editor with following goals:

* Not a DAW: MIDI is the input, MIDI is exported (I would like it to be but will not have enough time for that).
* Do not care (much) about measures. Primarily aimed at piano real time recordings without explicit tempo/bars.
* A feature absent in other midi editors I could get my hands on (both commercial and free ones): removing a piece
  of MIDI recording as one can remove a time fragment from PCM recording. For some odd reason DAW authors insist on
  handling time in MIDI editors differently from PCM sound recordings. In some editors this is doable but cumbersome at
  best.
* Playing/editing very long (at elast up to about 25K of playable events) files. Those files are usually recordings of
  real performances (e.g. from a MIDI keyboard).
* Comfortable workflow with keyboard.
* Allows making fine adjustments of notes and tempo.
* Unlimited undo/redo. Never loose session data. Non destructive edits, do not override original files.
* Flight recorder (always-on MIDI recording).

I'd love this to be in one of commercial or open-source DAWs and pay money for that, but I do not see it happening.
Ardour DAW can more or less do the trick, however inconveniently, but losing hours of work in a crash was not a good UX.

## Current status

Somewhat usable, no documentation yet (e.g. hot-keys, launching need to be described). The code still needs major
revamps and polishing. No backward compatibility guarantees. See TODO list below to se missing or planned parts.

## Requirements

Currently only Linux-like systems with ALSA are supported. Volunteers to support other platforms are welcome.

## Usage hints

**Note** that the program functions only as MIDI editor and uses a MIDI sequencer port for playback.
To get audible playback you should use some other software that can listen to the MIDI output port and produce some
sound. Any DAW or a synthesiser plugin with stand-alone UI would do. The output MIDI sequencer port name is
also `emmate`.

Currently expected usage scenario is opening a MIDI file passed as command line argument. This will create an `*.emmate`
folder with the same name containing editing history and exported MIDI files. For example

```shell
emmate --midi-file my-unpolished-masterpiece.mid
```

Run `emmate --help` to see other available switches.

All editing actions are persisted immediately, no need to do anything special to save your work. To export the stave to
a midi file press Ctrl+S. Exported file will be saved into the `*.emmate/export/`
folder.

Undo/redo history is unlimited.

You can transpose/correct loudness/shift/adjust length of selected notes. Draw/delete notes or complete time slices.
Draw on/off sustain pedal state (on the bottom lane).

Mouse zoom and scroll is supported. "Follow playback" switch makes the stave to scroll during playback.
You can set/clear bookmarks with `m`/`n`.

See `Stave::handle_commands` (src/stave.rs) method for availiable keyboard shortcuts.

## Build

In case you get "No package 'freetype2' found" on Linux
`apt install libxft-dev`.

ALSA wrapper dependency (used for MIDI input)
`apt install libasound2-dev`.

As an VST synth plugin you can use `amsynth`, for example.
I use Pianoteq, but that is a commercial product.

## TODO

- [ ] (refactoring, a big one) Use events ordered by note lanes, use MIDI events (or some equivalent) directly, instead
  of note ranges.
- [ ] (refactoring) Reduce number of range types (prefer util::Range?)
- [ ] Adjust tempo for a selection.
- [ ] Time marks on stave ("minute:second" from the beginning).
- [ ] Multi-track UI (for snippets, flight recorder, and copy/paste buffer). Can show only one at a time, though. Use
  tabs?
- [ ] Copy/cut/paste notes and time ranges (should also be supported between tracks).
- [ ] Zoom to fit whole composition.
- [ ] The sustain lane is left in an inconsistent state sometines (needs investigation).
- [ ] Location history navigation (e.g. go to a bookmark that was visited recently), with Alt + LeftArrow / RightArrow
- [ ] (refactoring) Organize commands (keep hotkeys/actions in a collection or registry). This should make the
  handle_commands easier to read and enable to have a generated cheatsheet/help UI.
- [ ] Recording events from the input sequencer (should probably use copy-paste to/from another track for overdubs for
  now).
- [ ] "Flight recorder", always record what is coming from the MIDI controller into a separate file or track.
- [ ] (improvement) Ensure changes are visible even when zoomed out (the events may be less than 1 pixel in size to be
  visible as is).
- [x] (refactoring) Minimize use of unwrap. The biggest contention currently is event data shared between
  engine and stave. Maybe can do this with async or sending update commands to the engine thread (e.g. can just swap to
  new track copy in the engine's events source after edits).
- [x] Shift markers on time deletion to follow the marked timestamps.
- [x] (improvement) When start playing send current CC values (will help damper to take effect immediately, not on next
  change).
- [x] Remove VST2 dependency. Use MIDI sequencer port instead.
- [x] Highlight undo/redo changes (implemented for notes, need also to emphasise CC values).
- [x] Visual hint for out-of-view selected notes. Scroll to the earliest of the selected notes on an action, if none of
  them are currently visible.
- [x] Optimize undo history 2: save only minimal diff instead of the whole track.
- [x] Show (scroll to) changing objects before undo/redo. Should scroll some changes into view before animation on an
  undo/redo command if none are currently visible.
- [x] Reduce diff disk usage.
- [x] Consider TransportTime to be signed (see also StaveTime). There are too many conversions forth and back.
- [x] Persist bookmarks in project.
- [x] Have a separate edit-position and play-start cursors (time bookmarks), so it is easier to jump back and listen to
  the modified version.
- [x] Optimize undo history: avoid O(N) algos; batch fast similar commands (e.g. tail or note shifts) saving at most
  2-3 snapshots a second per streak; do not save a new snapshot when there are no changes.
- [x] Automatically create an undo snapshot on every edit command.
- [x] Select none/clear selection command
- [x] Editing sustain events.
- [x] Note input (with mouse).
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

Also, some big scary problems

* May need to use midi events directly (instead of intermediate internal representation). E.g. `track::from_midi_events`
  may not be necessary. In particular tail shifts will become simpler. This will require
    * To handle ignored/unused events along with notes and sustain.
    * Midi events have starting times relative to previous ones. May need some indexing mechanism (e.g. a range tree)
      that would help to find absolute timings of the midi events, and connect beginnings and endings of notes. MIDI
      allows overlapping notes index should be able to handle that.
* Optimize rendering drawing only visible items, may also need some index. In the simplest casevisible notes can be
  determined when zoom changes, and then re-use the visible set.
* Optimize engine to reduce CPU usage - may need to switch to some async framework (`tokio`).

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

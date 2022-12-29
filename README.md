# midired

Off grid midi editor with following goals:

* A feature absent in _any_ other midi editor I could get my hands on (both commercial and free ones): removing a note
  can shift the tail of the track left to fill the gap. In some editors this is actually doable but cumbersome at best.
* Playing/editing very long (over several thousands events) files.
  Those files are usually recordings of real performance (e.g. from MIDI keyboard).
* Comfortable workflow with keyboard as primary input.
* Primarily aimed at real recordings without strict tempo/bars.

## Status

Not even a prototype, still figuring things out.
I am learning both Rust, MIDI and sound processing at once so the code should not be expected to be a good style example
for now.

## Build

In case you get "No package 'freetype2' found" on Linux
`apt install libxft-dev`.

ALSA wrapper dependency
`apt install libasound2-dev`.

As an example synth plugin you can use `amsynth`.

## TODO

For a prototype

* A UI window with simple text message log and quit menu, hotkey.
* Load/decode a MIDI file
* Load a MIDI->PCM plugin (https://github.com/RustAudio/vst-rs/blob/master/examples/simple_host.rs)
* Play MIDI
* Show MIDI pane
  ** Piano roll
  ** Paint notes

For prototype version use hard-coded MIDI filename and VST path.

May explore following options for the next version

* Use [Tokio](https://github.com/tokio-rs/tokio) for scheduling instead of spinning in a thread.
* Or, be based on [Dropseed](https://github.com/MeadowlarkDAW/dropseed) (which is used in MeadowlarkDAW)
* Or, ideally, participate in [MeadowlarkDAW](https://github.com/MeadowlarkDAW/Meadowlark) - but I am not comfortable to
  take on that yet. 
        
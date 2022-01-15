# midired

Off grid midi editor with following goals:
* Playing/editing very long (over several thousands events) files.
Those files are usually recordings of real performance (e.g. from MIDI keyboard).
* Comfortable workflow with keyboard as primary input.
* Primarily aimed at real recordings without strict tempo/bars. 

## Status

Not even a prototype, figuring things out.

## Build

In case you get "No package 'freetype2' found" on Linux
`apt install libxft-dev`.

ALSA wrapper dependency
`apt install libasound2-dev`.

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


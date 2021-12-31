# midired

Off grid midi editor with following goals:
* Playing/editing very long (over several thousands events) files.
Those files are usually recordings of real performance (e.g. from MIDI keyboard).
* Comfortable workflow with keyboard as primary input.
* Primarily aimed at real recordings without strict tempo/bars. 

## Build

Requires GTK3 (see https://github.com/linebender/druid/blob/master/README.md):
```sh
apt-get install libgtk-3-dev
```

## TODO

For a prototype

* A UI window with simple text message log and quit menu, hotkey.
* Load/decode a MIDI file
* Load a MIDI->PCM plugin (https://github.com/RustAudio/vst-rs/blob/master/examples/simple_host.rs)
* Play MIDI
* Show MIDI pane
** Piano roll
** Paint notes

FOr prototype version use hard-coded MIDI filename and VST path. 


use crate::changeset::{Changeset, EventAction};
use crate::common::Time;
use crate::track::{
    is_cc_switch_on, ControllerId, ControllerSetValue, Level, Note, Pitch, Track, TrackEvent,
    TrackEventType, MAX_LEVEL, MIDI_CC_SUSTAIN_ID,
};
use crate::util;
use crate::util::{range_contains, IdSeq};

pub fn add_note(
    track: &mut Track,
    changeset: &mut Changeset,
    time_range: (Time, Time),
    pitch: Pitch,
    level: Level,
) {
    changeset.add(EventAction::Insert(TrackEvent {
        id: track.id_seq.next(),
        at: time_range.0,
        event: TrackEventType::Note(Note {
            pitch,
            velocity: level,
            duration: time_range.1 - time_range.0,
        }),
    }));
}

fn clear_cc_events(
    track: &mut Track,
    changeset: &mut Changeset,
    time_range: util::Range<Time>,
    cc_id: ControllerId,
) {
    for ev in &track.events {
        if range_contains(time_range, ev.at) {
            if let TrackEventType::Controller(cc) = &ev.event {
                if cc.controller_id == cc_id {
                    changeset.add(EventAction::Delete(ev.clone()));
                }
            }
        }
    }
}

fn sustain_event(id_seq: &mut IdSeq, at: &Time, on: bool) -> TrackEvent {
    TrackEvent {
        id: id_seq.next(),
        at: *at,
        event: TrackEventType::Controller(ControllerSetValue {
            controller_id: MIDI_CC_SUSTAIN_ID,
            value: if on { MAX_LEVEL } else { 0 },
        }),
    }
}

pub fn set_damper_to(
    track: &mut Track,
    changeset: &mut Changeset,
    time_range: util::Range<Time>,
    on: bool,
) {
    dbg!("set_damper", time_range, on);
    let on_before = is_cc_switch_on(cc_value_at(
        &track.events,
        &time_range.0,
        &MIDI_CC_SUSTAIN_ID,
    ));
    let on_after = is_cc_switch_on(cc_value_at(
        &track.events,
        &time_range.1,
        &MIDI_CC_SUSTAIN_ID,
    ));

    clear_cc_events(track, changeset, time_range, MIDI_CC_SUSTAIN_ID);

    if on {
        if !on_before {
            let on_ev = sustain_event(&mut track.id_seq, &time_range.0, true);
            changeset.add(EventAction::Insert(on_ev));
        }
        if !on_after {
            let off_ev = sustain_event(&mut track.id_seq, &time_range.1, false);
            changeset.add(EventAction::Insert(off_ev));
        }
    } else {
        if on_before {
            let off_ev = sustain_event(&mut track.id_seq, &time_range.0, false);
            changeset.add(EventAction::Insert(off_ev));
        }
        if on_after {
            let on_ev = sustain_event(&mut track.id_seq, &time_range.1, true);
            changeset.add(EventAction::Insert(on_ev));
        }
    }
}

fn cc_value_at(events: &Vec<TrackEvent>, at: &Time, cc_id: &ControllerId) -> Level {
    let mut idx = events.partition_point(|x| x.at < *at);
    while idx > 0 {
        idx -= 1;
        if let Some(ev) = events.get(idx) {
            if let TrackEventType::Controller(cc) = &ev.event {
                if cc.controller_id == *cc_id {
                    return cc.value;
                }
            }
        }
    }
    return 0; // default
}

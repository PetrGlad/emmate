// TODO (cleanup) All functions in this module should accept only immutable track.
//   Review remaining usages that require mut Track.

use std::collections::HashSet;

use crate::changeset::{Changeset, EventAction, EventFn};
use crate::common::Time;
use crate::track::{
    ControllerId, ControllerSetValue, EventId, is_cc_switch_on, Level, MAX_LEVEL, MIDI_CC_SUSTAIN_ID, Note,
    Pitch, Track, TrackEvent, TrackEventType,
};
use crate::util;
use crate::util::{IdSeq, range_contains};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TimeSelection {
    pub from: Time,
    pub to: Time,
}

impl TimeSelection {
    pub fn length(&self) -> Time {
        self.to - self.from
    }

    pub fn contains(&self, at: Time) -> bool {
        self.from <= at && at < self.to
    }

    pub fn before(&self, at: Time) -> bool {
        self.to <= at
    }

    pub fn after_start(&self, at: Time) -> bool {
        self.from <= at
    }

    pub fn is_empty(&self) -> bool {
        self.to - self.from <= 0
    }
}

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

pub fn edit_events<Selector: Fn(&TrackEvent) -> bool>(
    track: &mut Track,
    changeset: &mut Changeset,
    selector: &Selector,
    action: &EventFn,
) {
    for ev in &mut track.events {
        if selector(&ev) {
            if let Some(a) = action(&ev) {
                changeset.add(a);
            }
        }
    }
}

pub fn delete_events(track: &mut Track, changeset: &mut Changeset, event_ids: &HashSet<EventId>) {
    edit_events(track, changeset, &|ev| event_ids.contains(&ev.id), &|ev| {
        Some(EventAction::Delete(ev.clone()))
    });
}

pub fn tape_cut(track: &mut Track, changeset: &mut Changeset, time_selection: &TimeSelection) {
    dbg!("tape_cut", time_selection);
    edit_events(
        track,
        changeset,
        &|ev| time_selection.contains(ev.at),
        &|ev| Some(EventAction::Delete(ev.clone())),
    );
    shift_events(
        track,
        changeset,
        &|ev| time_selection.before(ev.at),
        -(time_selection.length() as i64),
    );
}

pub fn shift_events<Pred: Fn(&TrackEvent) -> bool>(
    track: &mut Track,
    changeset: &mut Changeset,
    selector: &Pred,
    d: i64,
) {
    edit_events(track, changeset, selector, &move |ev| {
        let mut nev = ev.clone();
        nev.at += d;
        Some(EventAction::Update(ev.clone(), nev))
    });
}

pub fn tape_insert(track: &mut Track, changeset: &mut Changeset, time_selection: &TimeSelection) {
    dbg!("tape_insert", time_selection);
    shift_events(
        track,
        changeset,
        &|ev| time_selection.after_start(ev.at),
        time_selection.length() as i64,
    );
}

pub fn shift_tail(track: &mut Track, changeset: &mut Changeset, at: &Time, dt: i64) {
    dbg!("tail_shift", at, dt);
    shift_events(track, changeset, &|ev| &ev.at >= at, dt);
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

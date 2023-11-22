// TODO (cleanup) All functions in this module should accept only immutable track.
//   It is possible to not use id generator here, by having some id-placeholder value.

use std::collections::HashSet;

use crate::changeset::{Changeset, EventAction, EventFn};
use crate::common::Time;
use crate::track::{
    is_cc_switch_on, ControllerId, ControllerSetValue, EventId, Level, Note, Pitch, Track,
    TrackEvent, TrackEventType, MAX_LEVEL, MIDI_CC_SUSTAIN_ID,
};
use crate::util;
use crate::util::{range_contains, IdSeq};

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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_track() -> Track {
        let mut events: Vec<TrackEvent> = vec![];
        events.push(TrackEvent {
            id: 10,
            at: 10,
            event: TrackEventType::Controller(ControllerSetValue {
                controller_id: 13,
                value: 55,
            }),
        });
        events.push(TrackEvent {
            id: 20,
            at: 14,
            event: TrackEventType::Note(Note {
                pitch: 10,
                velocity: 20,
                duration: 30,
            }),
        });
        events.push(TrackEvent {
            id: 30,
            at: 15,
            event: TrackEventType::Controller(ControllerSetValue {
                controller_id: 44,
                value: 60,
            }),
        });
        events.push(TrackEvent {
            id: 40,
            at: 20,
            event: TrackEventType::Controller(ControllerSetValue {
                controller_id: 13,
                value: 66,
            }),
        });
        let mut track = Track::default();
        track.events = events;
        track
    }

    #[test]
    fn check_cc_value_at() {
        let track = make_test_track();

        assert_eq!(55, cc_value_at(&track.events, &20, &13));
        assert_eq!(66, cc_value_at(&track.events, &21, &13));
        assert_eq!(60, cc_value_at(&track.events, &21, &44));
        assert_eq!(0, cc_value_at(&track.events, &21, &99));
        assert_eq!(0, cc_value_at(&track.events, &0, &99));
    }

    #[test]
    fn check_set_damper_to() {
        let mut track = make_test_track();
        let mut changeset = Changeset::empty();
        set_damper_to(&mut track, &mut changeset, (13, 17), true);
        track.patch(&changeset);

        let expected_ids: Vec<EventId> = vec![10, 0, 20, 30, 1, 40];
        assert_eq!(
            expected_ids,
            track
                .events
                .iter()
                .map(|ev| ev.id)
                .collect::<Vec<EventId>>()
        );

        let expected_states: Vec<Option<bool>> = vec![
            Some(false),
            Some(true),
            None,
            Some(false),
            Some(false),
            Some(true),
        ];
        assert_eq!(
            expected_states,
            track
                .events
                .iter()
                .map(|ev| if let TrackEventType::Controller(ctl) = &ev.event {
                    Some(is_cc_switch_on(ctl.value))
                } else {
                    None
                })
                .collect::<Vec<Option<bool>>>()
        );
    }
}

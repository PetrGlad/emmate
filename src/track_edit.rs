use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::changeset::{Changeset, EventAction, EventActionsList};
use crate::common::Time;
use crate::stave::PIANO_KEY_LINES;
use crate::track::{
    is_cc_switch_on, ControllerId, ControllerSetValue, EventId, Level, Note, Pitch, Track,
    TrackEvent, TrackEventType, MAX_LEVEL, MIDI_CC_SUSTAIN_ID,
};
use crate::util::{range_contains, IdSeq, Range};

#[derive(Serialize, Deserialize)]
pub enum EditCommandId {
    ShiftTail,
    TapeInsert,
    TapeDelete,
    AddNote,
    DeleteEvents,
    SetDamper,
    SetDamperOn,
    EventsShift,
    NotesStretch,
    NotesTranspose,
    NotesAccent,
    Load,
}

// Commands that do not usually generate large patches can use generic Changeset, this is the default.
// Commands that cannot be stored efficiently should use custom diffs.
// Note to support undo, the event updates must be reversible.
#[derive(Serialize, Deserialize)]
pub enum CommandDiff {
    Changeset { patch: EventActionsList },
    TailShift { at: Time, delta: Time },
}

pub type AppliedCommand = (EditCommandId, Vec<CommandDiff>);

pub fn apply_diffs(track: &mut Track, diffs: &Vec<CommandDiff>) {
    for d in diffs {
        let mut changeset = Changeset::empty();
        apply_diff(track, d, &mut changeset);
        track.patch(&changeset);
    }
}

pub fn apply_diff(track: &Track, diff: &CommandDiff, changeset: &mut Changeset) {
    match diff {
        CommandDiff::Changeset { patch } => changeset.add_all(patch),
        CommandDiff::TailShift { at, delta } => do_shift_tail(track, at, &delta, changeset),
    }
}

pub fn revert_diffs(track: &mut Track, diffs: &Vec<CommandDiff>) {
    for d in diffs.iter().rev() {
        let mut changeset = Changeset::empty();
        revert_diff(track, d, &mut changeset);
        track.patch(&changeset);
    }
}

pub fn revert_diff(track: &Track, diff: &CommandDiff, changeset: &mut Changeset) {
    match diff {
        CommandDiff::Changeset { patch } => {
            for action in patch.iter().rev() {
                changeset.add(action.revert())
            }
        }
        CommandDiff::TailShift { at, delta } => do_shift_tail(track, at, &-delta, changeset),
    }
}

pub fn shift_tail(at: &Time, delta: &Time) -> (EditCommandId, Vec<CommandDiff>) {
    (
        EditCommandId::ShiftTail,
        vec![CommandDiff::TailShift {
            at: *at,
            delta: *delta,
        }],
    )
}

// TODO (cleanup) Organize the naming? Functions updating changeset, vs functions producing CommandDiffs
fn do_shift_tail(track: &Track, at: &Time, delta: &Time, changeset: &mut Changeset) {
    // TODO (improvement) When moving earlier adjust last delta so the events will start exactly at 'at'.
    let mut actions = vec![];
    for ev in &track.events {
        if *at < ev.at {
            /* The action should be replayable and undoable. If we allow events to move earlier
            then 'at' time, then on undo we should somehow find them still while not confusing them
            with unchanged events in (at - delta, at] range (when if delta > 0).
            Track events are expected to be in sorted order, so this will likely trigger early.
            TODO (implementation) This is a normal situation, should just not apply the command,
              and maybe hint user that we hit the wall already. Need a way to interrupt this
              gracefully, or do the check before applying the command diff.
             */
            assert!(
                *at <= (ev.at + delta),
                "the shift_tail is not undoable at={}, delta={}",
                at,
                delta
            );

            actions.push(shift_event(&ev, delta));
        }
    }
    changeset.add_all(&actions);
}

fn do_delete_events_range(track: &Track, range: &Range<Time>, changeset: &mut Changeset) {
    for ev in &track.events {
        if range_contains(range, ev.at) {
            changeset.add(EventAction::Delete(ev.clone()));
        }
    }
}

pub fn tape_insert(range: &Range<Time>) -> AppliedCommand {
    let mut diffs = vec![];
    let delta = range.1 - range.0;
    assert!(delta >= 0);
    diffs.push(CommandDiff::TailShift { at: range.0, delta });
    (EditCommandId::TapeInsert, diffs)
}

pub fn tape_delete(track: &Track, range: &Range<Time>) -> AppliedCommand {
    let delta = range.1 - range.0;
    assert!(delta >= 0);
    let mut patch = vec![];
    for ev in &track.events {
        if range_contains(range, ev.at) {
            patch.push(EventAction::Delete(ev.clone()));
        }
    }
    (
        EditCommandId::TapeInsert,
        vec![
            CommandDiff::Changeset { patch },
            CommandDiff::TailShift {
                at: range.0,
                delta: -delta,
            },
        ],
    )
}

fn edit_selected(
    track: &Track,
    selection: &HashSet<EventId>,
    action: &dyn Fn(&TrackEvent) -> Option<EventAction>,
) -> Vec<CommandDiff> {
    let mut patch = vec![];
    for ev in &track.events {
        if selection.contains(&ev.id) {
            if let Some(action) = action(&ev) {
                patch.push(action);
            }
        }
    }
    vec![CommandDiff::Changeset { patch }]
}

fn edit_selected_notes<'a, Action: Fn(&Note) -> Option<Note>>(
    track: &Track,
    selection: &HashSet<EventId>,
    action: &'a Action,
) -> Vec<CommandDiff> {
    // Adapt note action to be an event action.
    let event_action = move |ev: &TrackEvent| {
        if let TrackEvent {
            event: TrackEventType::Note(n),
            ..
        } = &ev
        {
            if let Some(n) = action(n) {
                let mut ev = ev.clone();
                ev.event = TrackEventType::Note(n);
                return Some(EventAction::Update(ev.clone(), ev));
            }
        }
        None
    };
    edit_selected(track, selection, &event_action)
}

pub fn delete_selected(track: &Track, selection: &HashSet<EventId>) -> AppliedCommand {
    let diff = edit_selected(track, selection, &|ev| {
        Some(EventAction::Delete(ev.clone()))
    });
    (EditCommandId::DeleteEvents, diff)
}

fn shift_event(ev: &TrackEvent, delta: &Time) -> EventAction {
    let mut nev = ev.clone();
    nev.at += delta;
    EventAction::Update(ev.clone(), nev)
}

pub fn shift_selected(track: &Track, selection: &HashSet<EventId>, delta: &Time) -> AppliedCommand {
    let diff = edit_selected(track, selection, &|ev| Some(shift_event(ev, delta)));
    (EditCommandId::EventsShift, diff)
}

pub fn stretch_selected_notes(
    track: &Track,
    selection: &HashSet<EventId>,
    delta: &Time,
) -> AppliedCommand {
    let diff = edit_selected_notes(track, selection, &|note: &Note| {
        let mut note = note.clone();
        note.duration += delta;
        Some(note)
    });
    (EditCommandId::NotesStretch, diff)
}

pub fn transpose_selected_notes(
    track: &Track,
    selection: &HashSet<EventId>,
    delta: i8,
) -> AppliedCommand {
    let diff = edit_selected_notes(track, selection, &|note: &Note| {
        let mut note = note.clone();
        if let Some(x) = note.pitch.checked_add_signed(delta) {
            if PIANO_KEY_LINES.contains(&x) {
                note.pitch = x;
                return Some(note);
            }
        }
        None
    });
    (EditCommandId::NotesTranspose, diff)
}

pub fn accent_selected_notes(
    track: &Track,
    selection: &HashSet<EventId>,
    delta: i8,
) -> AppliedCommand {
    let diff = edit_selected_notes(track, selection, &|note: &Note| {
        if let Some(pitch) = note.velocity.checked_add_signed(delta) {
            let mut note = note.clone();
            note.velocity = pitch;
            Some(note)
        } else {
            None
        }
    });
    (EditCommandId::NotesAccent, diff)
}

pub fn add_new_note(id_seq: &IdSeq, range: &Range<Time>, pitch: &Pitch) -> AppliedCommand {
    let mut diff = vec![];
    assert!(range.1 - range.0 > 0);
    diff.push(CommandDiff::Changeset {
        patch: vec![EventAction::Insert(TrackEvent {
            id: id_seq.next(),
            at: range.0,
            event: TrackEventType::Note(Note {
                pitch: *pitch,
                velocity: MAX_LEVEL / 2,
                duration: range.1 - range.0,
            }),
        })],
    });
    (EditCommandId::AddNote, diff)
}

fn sustain_event(id_seq: &IdSeq, at: &Time, on: bool) -> TrackEvent {
    TrackEvent {
        id: id_seq.next(),
        at: *at,
        event: TrackEventType::Controller(ControllerSetValue {
            controller_id: MIDI_CC_SUSTAIN_ID,
            value: if on { MAX_LEVEL } else { 0 },
        }),
    }
}

pub fn set_damper(id_seq: &IdSeq, track: &Track, range: &Range<Time>, on: bool) -> AppliedCommand {
    let mut patch = vec![];
    let on_before = is_cc_switch_on(cc_value_at(&track.events, &range.0, &MIDI_CC_SUSTAIN_ID));
    let on_after = is_cc_switch_on(cc_value_at(
        &track.events,
        &(range.1 + 1),
        &MIDI_CC_SUSTAIN_ID,
    ));

    clear_cc_events(track, range, MIDI_CC_SUSTAIN_ID, &mut patch);
    if on {
        if !on_before {
            let on_ev = sustain_event(&id_seq, &range.0, true);
            patch.push(EventAction::Insert(on_ev));
        }
        if !on_after {
            let off_ev = sustain_event(&id_seq, &range.1, false);
            patch.push(EventAction::Insert(off_ev));
        }
    } else {
        if on_before {
            let off_ev = sustain_event(&id_seq, &range.0, false);
            patch.push(EventAction::Insert(off_ev));
        }
        if on_after {
            let on_ev = sustain_event(&id_seq, &range.1, true);
            patch.push(EventAction::Insert(on_ev));
        }
    }

    (
        EditCommandId::SetDamper,
        vec![CommandDiff::Changeset { patch }],
    )
}

fn clear_cc_events(
    track: &Track,
    range: &Range<Time>,
    cc_id: ControllerId,
    patch: &mut Vec<EventAction>,
) {
    for ev in &track.events {
        if range_contains(range, ev.at) {
            if let TrackEventType::Controller(cc) = &ev.event {
                if cc.controller_id == cc_id {
                    patch.push(EventAction::Delete(ev.clone()));
                }
            }
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
        let id_seq = IdSeq::new(0);
        let applied_command = set_damper(&id_seq, &track, &(13, 17), true);
        apply_diffs(&mut track, &applied_command.1);

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

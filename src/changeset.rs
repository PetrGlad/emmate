use std::collections::HashMap;

use crate::track::{EventId, Note, TrackEvent, TrackEventType};

/// Simplest track edit operation. See Changeset for uses.
#[derive(Debug, PartialEq, Eq)]
pub enum EventAction {
    // TODO It is possible to recover necessary state by patching one of the recent (preferably the
    //   most recent) snapshots. Such snapshots (the ones that track event ids) are not
    //   implemented yet, so adding "before" states here to support undo operations
    //   as the initial draft in-memory implementation.
    Delete(TrackEvent),
    Update(TrackEvent, TrackEvent),
    Insert(TrackEvent),
}

impl EventAction {
    pub fn event_id(&self) -> EventId {
        match self {
            EventAction::Delete(ev) => ev.id,
            EventAction::Update(_, ev) => ev.id,
            EventAction::Insert(ev) => ev.id,
        }
    }

    pub fn after(&self) -> Option<&TrackEvent> {
        match self {
            EventAction::Delete(_) => None,
            EventAction::Update(_, ev) => Some(ev),
            EventAction::Insert(ev) => Some(ev),
        }
    }

    pub fn before(&self) -> Option<&TrackEvent> {
        match self {
            EventAction::Delete(ev) => Some(ev),
            EventAction::Update(ev, _) => Some(ev),
            EventAction::Insert(_) => None,
        }
    }
}

/// Complete patch of a track edit action.
/// TODO This should be a part of the persisted edit history, then it should contain the complete event values instead of ids.
///   Note that this would also require event ids that are unique within the whole project history (the generator value should be)
#[derive(Debug, Default)]
pub struct Changeset {
    pub changes: HashMap<EventId, EventAction>,
}

impl Changeset {
    pub fn put(&mut self, action: EventAction) {
        self.changes.insert(action.event_id(), action);
    }

    pub fn merge(&mut self, other: Self) {
        self.changes.extend(other.changes);
    }
}

pub type EventFn = dyn Fn(&TrackEvent) -> Option<EventAction> + 'static;

pub struct UpdateCommand {
    id: String,
    proc: Box<EventFn>,
}

/// Convenience wrapper
pub fn event_to_note_action<NoteFn: Fn(&Note) -> Option<Note> + 'static>(
    action: NoteFn,
) -> Box<EventFn> {
    Box::new(move |ev| {
        if let TrackEvent {
            event: TrackEventType::Note(n),
            ..
        } = &ev
        {
            match action(n) {
                Some(note) => Some(EventAction::Update(
                    ev.clone(),
                    TrackEvent {
                        id: ev.id,
                        at: ev.at,
                        event: TrackEventType::Note(note),
                    },
                )),
                None => None,
            }
        } else {
            None
        }
    })
}

use std::collections::HashMap;

use eframe::egui::KeyboardShortcut;

use crate::track::{EventId, Note, TrackEvent, TrackEventType};

#[derive(Debug, PartialEq, Eq)]
pub enum EventAction {
    Delete(EventId),
    Update(TrackEvent),
    Insert(TrackEvent),
}

impl EventAction {
    pub fn event_id(&self) -> EventId {
        match self {
            EventAction::Delete(id) => *id,
            EventAction::Update(ev) => ev.id,
            EventAction::Insert(ev) => ev.id,
        }
    }
}

/// Resulting patch of a track edit action
/// TODO This should be a part of the persisted edit history, then it should contain the complete event values instead of ids.
///     Note that this would also require event ids that are unique within the whole project history (the generator value should be)
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

/// TODO Implement
pub enum ActionType {}

/// TODO Implement
pub struct StaveHotkeys {
    list: HashMap<KeyboardShortcut, ActionType>,
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
                Some(note) => Some(EventAction::Update(TrackEvent {
                    id: ev.id,
                    at: ev.at,
                    event: TrackEventType::Note(note),
                })),
                None => None,
            }
        } else {
            None
        }
    })
}

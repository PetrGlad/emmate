use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::common::VersionId;
use crate::track::{EventId, Track, TrackEvent};
use crate::track_edit::{CommandDiff, EditCommandType};

/// Simplest track edit operation. See [Changeset] for uses.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum EventAction {
    /* Adding "before" states here to support undo operations (the EventAction itself has
    enough information to undo).
    TODO (possible revamp) Alternatively it is also possible to recover necessary state by patching
     one of the recent snapshots. That approach may probably help to save space and simplify
     data structures. E.g. Delete action will only need the event id and update action will
     only need the new state. OTOH then we'll need to save snapshots more often. */
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

    pub fn before(&self) -> Option<&TrackEvent> {
        match self {
            EventAction::Delete(ev) => Some(ev),
            EventAction::Update(ev, _) => Some(ev),
            EventAction::Insert(_) => None,
        }
    }

    pub fn after(&self) -> Option<&TrackEvent> {
        match self {
            EventAction::Delete(_) => None,
            EventAction::Update(_, ev) => Some(ev),
            EventAction::Insert(ev) => Some(ev),
        }
    }

    pub fn revert(&self) -> Self {
        match self {
            EventAction::Delete(ev) => EventAction::Insert(ev.clone()),
            EventAction::Update(before, after) => {
                EventAction::Update(after.clone(), before.clone())
            }
            EventAction::Insert(ev) => EventAction::Delete(ev.clone()),
        }
    }
}

/// Complete patch of a track editing action.
/// Plain [EventActionsList] can be used instead, but there the actions order becomes important
/// (e.g. duplicating 'update' actions will overwrite previous result).
#[derive(Debug)]
pub struct Changeset {
    pub changes: HashMap<EventId, EventAction>,
}

pub type EventActionsList = Vec<EventAction>;

impl Changeset {
    pub fn empty() -> Self {
        Changeset {
            changes: HashMap::new(),
        }
    }

    pub fn add(&mut self, action: EventAction) {
        // Check for consistency, short circuit if possible.
        let id = action.event_id();
        let new_action = if let Some(prev_action) = self.changes.get(&id) {
            Self::merge_actions(id, &action, &prev_action)
        } else {
            Some(action)
        };

        if let Some(op) = new_action {
            self.changes.insert(id, op);
        }
    }

    fn merge_actions(
        id: EventId,
        action: &EventAction,
        prev_action: &EventAction,
    ) -> Option<EventAction> {
        // May not even need all the cases, no use-case for merging changesets ATM...
        // Just covering everything, for now, to see how it goes...
        use EventAction::*;
        match (&prev_action, &action) {
            (Insert(_), Insert(_)) => panic!("double insert, ev.id={}", id),
            (Insert(_), Update(_, b)) => Some(Insert(b.clone())),
            (Insert(_), Delete(_)) => None,

            (Update(_, _), Insert(_)) => panic!("inserting existing event, ev.id={}", id),
            (Update(a, _), Update(_, c)) => Some(Update(a.clone(), c.clone())),
            (Update(a, _), Delete(_)) => Some(Delete(a.clone())),

            (Delete(a), Insert(b)) => Some(Update(a.clone(), b.clone())),
            (Delete(_), Update(_, _)) => panic!("update of a deleted event, ev.id={}", id),
            (Delete(_), Delete(_)) => panic!("double delete, ev.id={}", id),
        }
    }

    pub fn add_all(&mut self, actions: &EventActionsList) {
        for a in actions.iter().cloned() {
            self.add(a);
        }
    }

    pub fn merge(&mut self, actions: &Changeset) {
        for a in actions.changes.values().cloned() {
            self.add(a);
        }
    }
}

/// Serializable changeset, diff. Storing these to keep whole edit history persistent, help with
/// undo hints (so it is obvious what is currently changing), and avoid storing whole track
/// every time. See also [Snapshot], [Changeset].
#[derive(Serialize, Deserialize)]
pub struct HistoryLogEntry {
    pub base_version: VersionId,
    pub version: VersionId,
    pub command_id: EditCommandType,
    pub diff: Vec<CommandDiff>,
}

/// Serializable snapshot of a complete track state that can be exported or used as a base
/// for Patch sequence. See also [HistoryLogEntry].
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub version: VersionId,
    pub events: Vec<TrackEvent>,
}

impl Snapshot {
    pub fn of_track(version: VersionId, track: &Track) -> Self {
        Snapshot {
            version,
            events: track.events.clone(),
        }
    }
}

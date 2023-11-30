use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::common::VersionId;
use crate::track::{EventId, Track, TrackEvent};
use crate::track_edit::{CommandDiff, EditCommandId};

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
        let id = action.event_id();
        if let Some(prev) = self.changes.insert(id, action) {
            // Check for consistency.
            use EventAction::*;
            match (&prev, &self.changes.get(&id).unwrap()) {
                // In theory these are OK (the latter just takes precedence) but not expected.
                (Insert(_), Insert(_)) => panic!("double insert patch, ev.id={}", id),
                (Delete(_), Delete(_)) => panic!("double delete patch, ev.id={}", id),
                // Likely CommandDiffs were not applied in the expected order.
                (Delete(_), Update(_, _)) => panic!("update of a deleted event, ev.id={}", id),
                (_, _) => (),
            }
        }
    }

    pub fn add_all(&mut self, actions: &EventActionsList) {
        for a in actions.iter().cloned() {
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
    pub command_id: EditCommandId,
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

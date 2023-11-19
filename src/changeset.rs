use crate::common::VersionId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::track::{EventId, Note, TrackEvent, TrackEventType};

/// Simplest track edit operation. See Changeset for uses.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn add(&mut self, action: EventAction) {
        self.changes.insert(action.event_id(), action);
    }

    pub fn merge(&mut self, other: Self) {
        self.changes.extend(other.changes);
    }
}

/// Serializable changeset, diff. Storing these to keep whole edit history persistent, help with
/// undo hints (so it is obvious what is currently changing), and avoid storing whole track
/// every time. See also [Snapshot], [Changeset].
#[derive(Serialize, Deserialize)]
pub struct Patch {
    pub base_version: VersionId,
    pub version: VersionId,
    pub changes: Vec<EventAction>,
}

impl Patch {
    pub fn load(&mut self, file_path: PathBuf) {
        todo!("load changeset from file");
    }

    pub fn store(&self, file_path: PathBuf) {
        todo!("load changeset from file");
    }
}

/// Serializable snapshot of a complete track state that can be exported or used as a base
/// for Patch sequence. See also [Patch].
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub version: VersionId,
    pub events: Vec<TrackEvent>,
}

impl Snapshot {
    pub fn load(&mut self, file_path: PathBuf) {
        todo!("load changeset from file");
    }

    pub fn store(&self, file_path: PathBuf) {
        todo!("load changeset from file");
    }
}

pub type EventFn = dyn Fn(&TrackEvent) -> Option<EventAction> + 'static;

pub struct UpdateCommand {
    id: String,
    proc: Box<EventFn>,
}

/// Convenience wrapper
pub fn to_event_action<NoteFn: Fn(&Note) -> Option<Note> + 'static>(
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

use std::cmp::Ordering;
// A sketch of new track events.
use crate::common::Time;
use crate::track::EventId;
use serde::{Deserialize, Serialize};

pub type Pitch = u8;
pub type ControllerId = u8;
/// CC level or note velocity.
pub type Level = u8;
pub type Velocity = Level;
/// Midi channel id.
pub type ChannelId = u8;

/// Serializable note event on the track.
#[derive(Default, Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub struct Tone {
    pub on: bool,
    pub pitch: Pitch,
    pub velocity: Velocity,
    /**
     Link to respective note end event, only for "on" events.
     This is only necessary when note edits make them intersecting.
     It is possible to apply some automatic resolution like cutting off the intersecting parts,
     but I'd prefer to let user decide what to do there.
     TODO (refactoring?) Ideally "on" and "off" should be separate cases, but then the pattern
        matching looks cumbersome.
     TODO (usability) When painting clearly mark intersecting note parts
        (with red or orange, maybe).
    */
    pub other: Option<EventId>,
}

/// Continuous Controller (CC) value set.
#[derive(Default, Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub struct Cc {
    pub controller_id: ControllerId,
    pub value: Level,
}

#[derive(Default, Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub struct Bookmark {}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub enum Type {
    Note(Tone),
    Cc(Cc),

    Bookmark(Bookmark),
}

/*
 TODO (refactoring) I would like to avoid explicit ids here as events can be uniquely identified by its position
   in the track (requires linear comparison ordering for ambiguous timestamps, though).
   Currently, the patch logic depends on it so to speed-up refactoring keeping id here for now.
*/
#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: EventId,
    pub at: Time,
    pub ev: Type,
}

impl Ord for Item {
    /**
        Ordering events by track time, while ensuring sorting always produces the same
        sequence every time. The time ordering is important for playback and editing while unique
        sort order ensures we do not have any surprises when changing or serializing the track.
    */
    fn cmp(&self, other: &Self) -> Ordering {
        (self.at, &self.ev, self.id).cmp(&(other.at, &other.ev, other.id))
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

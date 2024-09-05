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

#[derive(Default, Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub struct Tone {
    pub on: bool,
    pub pitch: Pitch,
    pub velocity: Velocity,
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
        Ordering events by track time, while ensuring sort order always produces the same
        result every time. The time ordering is important for playback and editing while unique
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

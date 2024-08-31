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

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub struct Tone {
    pub on: bool,
    pub pitch: Pitch,
    pub velocity: Velocity,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub struct Cc {
    pub controller_id: ControllerId,
    pub value: Level,
}

// Plan to use it in engine's track source.
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub enum Audio {
    Note(Tone),
    Cc(Cc),
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize)]
pub enum Type {
    Audio(Audio),
    Bookmark,
}

#[derive(Debug, Eq, PartialEq, Clone, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Item {
    /// I would like to avoid explicit ids here as events can be uniquely identified by its position
    /// in the track (requires linear comparison ordering for ambiguous timestamps, though).
    /// Currently, the patch logic depends on it so to speed-up refactoring keeping id here for now.
    pub id: EventId,
    pub at: Time,
    pub ev: Type,
}

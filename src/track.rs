use std::time::Duration;

pub type Pitch = u8;
pub type Velocity = u8;
pub type TrackTime = Duration;

#[derive(Debug)]
pub struct Note {
    pub pitch: Pitch,
    pub velocity: Velocity,
    // Since the track beginning.
    pub on: Duration,
    pub duration: Duration,
}

#[derive(Debug, Default)]
pub struct Track {
    pub notes: Vec<Note>
}

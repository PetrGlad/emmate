// A clipboard for exchanging track fragments between emmate instances.

use crate::common::{self, Time};
use crate::track::EventList;
use crate::util;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

/// Identifies the file type and its format version. Precedes the serialized data.
const FRAGMENT_MAGIC: &[u8] = b"emmate:fragment:v0:";

/// A piece of a track that was cut out by a time selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fragment {
    /// Length of the original time selection. May be longer than the events in the fragment.
    pub duration: Time,
    /// Event times are relative to the start of the original time selection.
    /// Events that began before the selection have negative times.
    pub events: EventList,
}

impl Fragment {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = FRAGMENT_MAGIC.to_vec();
        data.extend(util::encode(self, false));
        data
    }

    pub fn from_bytes(data: &[u8]) -> io::Result<Fragment> {
        let payload = data.strip_prefix(FRAGMENT_MAGIC).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "not an emmate fragment")
        })?;
        util::decode(payload)
    }
}

pub struct Clipboard {
    base_path: PathBuf,
}

impl Clipboard {
    const CLIPBOARD_DIR: &str = "clipboard";
    const FRAGMENT_FILE_NAME: &str = "track_fragment";

    pub fn init() -> Self {
        Clipboard {
            base_path: dirs::data_dir()
                .expect("clipboard directory path is not found")
                .join(common::APP_NAME)
                .join(Self::CLIPBOARD_DIR),
        }
    }

    pub fn fragment_file(&self) -> PathBuf {
        self.base_path.join(Self::FRAGMENT_FILE_NAME)
    }

    pub fn set_fragment(&self, fragment: &Fragment) -> io::Result<()> {
        std::fs::create_dir_all(&self.base_path)?;
        util::write_atomic(&self.fragment_file(), &fragment.to_bytes())
    }

    /// Fails with `ErrorKind::NotFound` when nothing was copied yet
    /// and with `ErrorKind::InvalidData` when the file has unexpected contents.
    pub fn get_fragment(&self) -> io::Result<Fragment> {
        Fragment::from_bytes(&std::fs::read(self.fragment_file())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{TrackEvent, TrackEventType};

    fn sample() -> Fragment {
        Fragment {
            duration: 2_000_000,
            events: vec![TrackEvent {
                id: 7,
                at: -5,
                event: TrackEventType::Bookmark,
            }],
        }
    }

    #[test]
    fn fragment_round_trip_keeps_duration_and_relative_times() {
        let fragment = sample();
        assert_eq!(fragment, Fragment::from_bytes(&fragment.to_bytes()).unwrap());
    }

    #[test]
    fn serialized_fragment_starts_with_magic() {
        assert!(sample().to_bytes().starts_with(b"emmate:fragment:v0:"));
    }

    #[test]
    fn fragment_without_magic_is_rejected() {
        let mut data = sample().to_bytes();
        data[7] = b'X';
        assert_eq!(
            io::ErrorKind::InvalidData,
            Fragment::from_bytes(&data).unwrap_err().kind()
        );
        let bare = util::encode(&sample(), false);
        assert!(Fragment::from_bytes(&bare).is_err());
    }

    #[test]
    fn truncated_fragment_is_rejected() {
        let data = sample().to_bytes();
        assert!(Fragment::from_bytes(&data[..data.len() - 5]).is_err());
        assert!(Fragment::from_bytes(b"").is_err());
    }
}

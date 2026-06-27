// A clippoard for exchanging track fragments between emmate instances.

use crate::common;
use crate::track::{EventList, TrackEvent};
use std::path::PathBuf;

pub struct Clipboard {
    base_path: PathBuf,

    // TODO Prototype implementation. Use shared persistent storage (file) instead.
    contents: EventList,
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
            contents: Vec::default(),
        }
    }

    fn fragment_file(&self) -> PathBuf {
        self.base_path.join(Self::FRAGMENT_FILE_NAME)
    }

    pub fn get_content(&self) -> Option<Vec<TrackEvent>> {
        Some(self.contents.to_owned())
    }

    pub fn set_content(&mut self, fragment: &EventList) {
        self.contents = fragment.to_owned();
    }
}

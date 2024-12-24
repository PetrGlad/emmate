// A clippoard for exchanging track fragments between emmate instances.

use crate::common;
use std::path::PathBuf;

const CLIPBOARD_DIR: &str = "clipboard";

pub struct Clipboard {
    base_path: PathBuf,
}

impl Clipboard {
    pub fn new() -> Self {
        Clipboard {
            base_path: dirs::data_dir()
                .expect("clipboard directory path is not found")
                .join(common::APP_NAME)
                .join(CLIPBOARD_DIR),
        }
    }

    pub fn get_latest(&self) -> String {
        todo!()
    }
}

use std::fs;
use std::path::PathBuf;

use crate::track_history::TrackHistory;

pub struct Project {
    pub history: TrackHistory,
    pub home_path: PathBuf,
}

impl Project {
    const DIRECTORY_NAME_SUFFIX: &'static str = "emmate";
    const HISTORY_DIR_NAME: &'static str = "history";

    pub fn open_file(source_file: &PathBuf) -> Project {
        dbg!("source file", source_file.to_string_lossy());
        let mut directory = source_file.to_owned();
        if directory.file_name().is_none() {
            panic!(
                "Source path has no file name: {}",
                directory.to_string_lossy()
            );
        }
        directory.set_extension("");
        directory.set_extension(Project::DIRECTORY_NAME_SUFFIX);

        let mut snapshots_dir = directory.clone();
        snapshots_dir.push(Self::HISTORY_DIR_NAME);

        let mut history = TrackHistory::with_directory(&snapshots_dir);
        if !snapshots_dir.is_dir() {
            fs::create_dir_all(&snapshots_dir).expect(
                format!("create project directory {:?}", directory.to_string_lossy()).as_str(),
            );
            history = history.init(&source_file)
        };
        history.open();
        Project {
            home_path: directory,
            history,
        }
    }
}

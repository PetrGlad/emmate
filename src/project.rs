use crate::track_history::TrackHistory;
use std::fs;
use std::path::PathBuf;

pub struct Project {
    pub history: TrackHistory,
    pub home_path: PathBuf,
}

impl Project {
    const DIRECTORY_NAME_SUFFIX: &'static str = "emmate";
    const SNAPSHOTS_DIR_NAME: &'static str = "snapshots";

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
        snapshots_dir.push(Self::SNAPSHOTS_DIR_NAME);

        let mut history = if snapshots_dir.is_dir() {
            TrackHistory::with_directory(&snapshots_dir)
        } else {
            fs::create_dir_all(&snapshots_dir).expect(
                format!("create project directory {:?}", directory.to_string_lossy()).as_str(),
            );
            TrackHistory::with_directory(&snapshots_dir).init(&source_file)
        };
        history.open();
        Project {
            home_path: directory,
            history,
        }
    }
}

use crate::track_history::TrackHistory;
use std::fs;
use std::path::PathBuf;

pub struct Project {
    pub history: TrackHistory,
    pub home_path: PathBuf,
}

impl Project {
    const DIRECTORY_NAME_SUFFIX: &'static str = "emmate";

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
        let mut history = if directory.is_dir() {
            TrackHistory::with_directory(&directory)
        } else {
            fs::create_dir_all(&directory).expect(
                format!(
                    "Cannot create project directory {:?}",
                    directory.to_string_lossy()
                )
                .as_str(),
            );
            TrackHistory::with_directory(&directory).init(&source_file)
        };
        history.open();
        Project {
            home_path: directory,
            history,
        }
    }
}

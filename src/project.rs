use crate::track_history::TrackHistory;
use std::cell::RefCell;
use std::error::Error;
use std::fs;
use std::path::{absolute, Path, PathBuf};

pub struct Project {
    pub title: String,
    pub history: RefCell<TrackHistory>,
    pub home_path: PathBuf,
}

impl Project {
    const DIRECTORY_NAME_SUFFIX: &'static str = "emmate";
    const HISTORY_DIR_NAME: &'static str = "history";

    pub fn open_file(source_file: &PathBuf) -> Project {
        log::info!("Source file {}", source_file.to_string_lossy());
        let mut directory = source_file.to_owned();
        if directory.file_name().is_none() {
            panic!(
                "Source path has no file name: {}",
                directory.to_string_lossy()
            );
        }
        directory.set_extension("");
        directory.set_extension(Project::DIRECTORY_NAME_SUFFIX);
        let directory = absolute(directory).expect("project directory path can be normalized");
        log::info!("Project directory {}", &directory.to_string_lossy());

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
            title: Self::path_to_title(&directory),
            home_path: directory,
            history: RefCell::new(history),
        }
    }

    // Clean the project path to make it less cluttered.
    fn path_to_title(project_path: &PathBuf) -> String {
        let mut result = project_path
            .canonicalize()
            .unwrap_or(project_path.to_owned());
        if let Some(hd) = dirs::home_dir() {
            result = result
                .strip_prefix(hd)
                .map(Path::to_path_buf)
                .unwrap_or(result)
        };
        result.set_extension("");
        result.to_string_lossy().to_string()
    }
}

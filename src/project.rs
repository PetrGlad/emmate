use crate::track_history::TrackHistory;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs;
use std::path::{absolute, Path, PathBuf};

// Persistence format version
pub const PROJECT_FORMAT_ID: u16 = 1;

pub struct Project {
    pub title: String,
    pub history: RefCell<TrackHistory>,
    pub home_path: PathBuf,
}

impl Project {
    const DIRECTORY_NAME_SUFFIX: &'static str = "emmate";
    const HISTORY_DIR_NAME: &'static str = "history";
    const META_FILE_NAME: &'static str = "meta.toml";

    // TODO (cleanup) Return a Result.
    pub fn init_from_midi_file(source_file: &PathBuf) -> PathBuf {
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
        fs::create_dir_all(&directory)
            .expect(format!("create project directory {:?}", directory.to_string_lossy()).as_str());
        let title = Self::path_as_title(&directory);
        let meta_path = directory.clone().join(Project::META_FILE_NAME);
        if !meta_path.is_file() {
            let meta = ProjectMeta {
                format_id: PROJECT_FORMAT_ID,
                git_revision: env!("GIT_HASH").into(),
                title: title.clone(),
                source_file: source_file.clone(),
            };
            Self::write_meta(&meta, &meta_path);
        }

        let mut history_dir = directory.clone();
        history_dir.push(Self::HISTORY_DIR_NAME);
        if !history_dir.is_dir() {
            fs::create_dir_all(&history_dir).expect(
                format!("create history directory {:?}", directory.to_string_lossy()).as_str(),
            );
            TrackHistory::with_directory(&history_dir).init(&source_file);
        };
        directory
    }

    pub fn open(project_path: &PathBuf) -> Project {
        let directory = absolute(project_path).expect("project directory path can be normalized");
        log::info!("Project directory {}", &directory.to_string_lossy());
        let meta_path = directory.clone().join(Project::META_FILE_NAME);
        let meta = Self::read_meta(&meta_path);
        Self::check_meta(&meta);

        let mut history_dir = directory.clone();
        history_dir.push(Self::HISTORY_DIR_NAME);
        let mut history = TrackHistory::with_directory(&history_dir);
        history.open();

        Project {
            title: meta.title,
            home_path: directory,
            history: RefCell::new(history),
        }
    }

    // Clean the project path to make it less cluttered.
    fn path_as_title(project_path: &PathBuf) -> String {
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

    // TODO (cleanup) Return a Result.
    fn check_meta(meta: &ProjectMeta) {
        if &meta.format_id != &PROJECT_FORMAT_ID {
            log::error!(
                "Incompatible project format. Supported {}, got {}. Refusing to overwrite.",
                PROJECT_FORMAT_ID,
                &meta.format_id
            );
            std::process::abort();
        }
    }

    fn read_meta(file_path: &PathBuf) -> ProjectMeta {
        let data =
            fs::read_to_string(&file_path).expect(&*format!("load from {}", &file_path.display()));
        let meta: ProjectMeta = toml::from_str(&data).expect("parse project metadata");
        log::info!("Read project meta {:?}", &meta);
        meta
    }

    fn write_meta(meta: &ProjectMeta, file_path: &PathBuf) {
        let data = toml::to_string(meta).expect("serialize project metadata");
        log::info!("Writing project metadata to {:?}", &file_path);
        fs::write(&file_path, data.as_bytes()).expect("write project metadata");
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub format_id: u16,
    pub git_revision: String,
    pub title: String,
    pub source_file: PathBuf,
}

impl ProjectMeta {
    fn new(source_file: &PathBuf) -> ProjectMeta {
        ProjectMeta {
            format_id: PROJECT_FORMAT_ID,
            git_revision: env!("GIT_HASH").into(),
            title: "".to_string(),
            source_file: source_file.to_owned(),
        }
    }
}

use std::fs;
use std::path::PathBuf;

use toml::from_str;

pub type VersionId = i64;

pub struct Project {
    pub directory: PathBuf,
    pub source_path: PathBuf,
    version: VersionId,
}

impl Project {
    const SNAPSHOT_NAME_EXT: &'static str = "emmrev.mid";
    const DIRECTORY_NAME_SUFFIX: &'static str = "emmate";

    pub fn version(&self) -> VersionId {
        self.version
    }

    pub fn new(source_file: &PathBuf) -> Project {
        let mut directory = source_file.to_owned();
        directory.set_extension(Project::DIRECTORY_NAME_SUFFIX);
        Project {
            directory,
            source_path: source_file.to_owned(),
            version: 0,
        }
    }

    pub fn open(&mut self) {
        if !self.directory.is_dir() {
            fs::create_dir_all(&self.directory).expect(
                format!(
                    "Cannot create project directory {:?}",
                    self.directory.display()
                )
                .as_str(),
            );
        }
        let starting_snapshot_path = self.current_snapshot_path();
        if !fs::metadata(&starting_snapshot_path).is_ok() {
            fs::copy(&self.source_path, &starting_snapshot_path)
                .expect("Cannot create starting snapshot.");
        } else {
            // Seek to the latest version.
            while fs::metadata(&self.current_snapshot_path()).is_ok() {
                self.change_version(1);
            }
            self.change_version(-1);
        }
    }

    pub fn parse_snapshot_name(file: &PathBuf) -> Option<VersionId> {
        let mut file = file.to_owned();
        if let Some(ext) = file.extension() {
            if ext != Project::SNAPSHOT_NAME_EXT {
                return None;
            }
            file.set_extension("");
            return from_str::<VersionId>(file.file_name().unwrap().to_str().unwrap()).ok();
        }
        None
    }

    fn make_snapshot_path(&self, version: VersionId) -> PathBuf {
        let mut path = self.directory.clone();
        path.push(format!("{:}.{:}", version, Project::SNAPSHOT_NAME_EXT));
        path
    }

    pub fn current_snapshot_path(&self) -> PathBuf {
        self.make_snapshot_path(self.version)
    }

    // version_diff < 0 for undo. version_diff == 1 - save new version.
    pub fn change_version(&mut self, version_diff: VersionId) -> Option<VersionId> {
        let new_version = self.version.checked_add(version_diff);
        new_version.filter(|v| *v >= 0).and_then(|v| {
            self.version = v;
            Some(v)
        })
    }
}

use crate::common::VersionId;
use crate::track::Track;
use glob::glob;
use regex::Regex;
use std::fs;
use std::path::PathBuf;

// Undo/redo history
pub struct TrackHistory {
    track: Track,
    pub version: VersionId,
    pub directory: PathBuf,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    id: VersionId,
    snapshot_path: PathBuf,
}

impl TrackHistory {
    const SNAPSHOT_NAME_EXT: &'static str = "emmrev.mid";

    /// Get current version.
    pub fn get(&self) -> &Track {
        todo!()
    }

    /// Save current version into history.
    pub fn push(&mut self) {
        todo!();
    }

    /// Restore track  from the last saved version.
    pub fn pop(&mut self) {
        todo!();
    }

    pub fn version(&self) -> VersionId {
        self.version
    }

    fn check_directory_writable(directory: &PathBuf) {
        let metadata = fs::metadata(&directory).expect(
            format!(
                "Cannot init history: {:} is not found.",
                &directory.to_string_lossy()
            )
            .as_str(),
        );
        if !metadata.is_dir() {
            panic!(
                "Cannot init history: {:} is not a directory.",
                &directory.to_string_lossy()
            );
        }
        if metadata.permissions().readonly() {
            panic!(
                "Cannot init history: {:} is not writable.",
                &directory.to_string_lossy()
            );
        }
    }

    pub fn with_directory(directory: &PathBuf) -> Self {
        dbg!("history directory", directory.to_string_lossy());
        Self::check_directory_writable(directory);
        Self {
            directory: directory.to_owned(),
            version: 0,
            track: Default::default(),
        }
    }

    pub fn init(self, source_file: &PathBuf) -> Self {
        if !self.is_empty() {
            panic!("Cannot init with new source file: the project history is not empty.")
        }
        assert_eq!(0, self.version);
        let starting_snapshot_path = self.current_snapshot_path();
        if fs::metadata(&starting_snapshot_path).is_ok() {
            panic!(
                "Not creating initial version: project history is not empty, '{:}' exists.",
                &starting_snapshot_path.to_string_lossy()
            );
        }
        fs::copy(&source_file, &starting_snapshot_path).expect("Cannot create starting snapshot.");
        self
    }

    pub fn open(&mut self) {
        dbg!(
            "revisions list",
            self.list_revisions().collect::<Vec<Version>>()
        );
        // Seek to the latest version.
        match self.list_revisions().last() {
            Some(v) => {
                self.change_version(v.id).unwrap();
            }
            None => panic!("No revision history in the project."),
        }
    }

    fn list_revisions(&self) -> impl Iterator<Item = Version> {
        let mut pattern = self.directory.to_owned();
        pattern.push("*.".to_string() + Self::SNAPSHOT_NAME_EXT);
        let files = glob(pattern.to_str().unwrap()).expect("List snapshots directory.");
        files
            // XXX `flatten` may hide metadata errors
            .flatten()
            .map(|p| {
                assert!(p.is_file());
                if let Some(id) = Self::parse_snapshot_name(&p) {
                    Some(Version {
                        id,
                        snapshot_path: Default::default(),
                    })
                } else {
                    None
                }
            })
            .flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.list_revisions().next().is_none()
    }

    pub fn parse_snapshot_name(file: &PathBuf) -> Option<VersionId> {
        let mut file = file.to_owned();
        let re = Regex::new((r"([0-9]+)\.".to_string() + Self::SNAPSHOT_NAME_EXT + "$").as_str())
            .unwrap();
        file.file_name()
            .and_then(|s| s.to_str())
            .and_then(|name| re.captures(&name))
            .and_then(|caps| caps.get(1))
            .and_then(|id_str| str::parse::<VersionId>(id_str.as_str()).ok())
    }

    fn make_snapshot_path(&self, version: VersionId) -> PathBuf {
        let mut path = self.directory.clone();
        path.push(format!("{:}.{:}", version, Self::SNAPSHOT_NAME_EXT));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_snapshot_name() {
        assert!(TrackHistory::parse_snapshot_name(&PathBuf::from("asdfsadf")).is_none());
        assert_eq!(
            Some(5),
            TrackHistory::parse_snapshot_name(&PathBuf::from("5.emmrev.mid"))
        );
    }
}

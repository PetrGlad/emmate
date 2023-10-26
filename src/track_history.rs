use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use glob::glob;
use regex::Regex;

use crate::common::VersionId;
use crate::track::Track;

// Undo/redo history and snapshots.
#[derive(Debug)]
pub struct TrackHistory {
    /// Normally should not be used from outside. Made it pub as double-borrow workaround.
    pub track: Arc<RwLock<Track>>,
    track_version: VersionId,
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

    pub fn with_track<Action: FnOnce(&Track)>(&mut self, action: Action) {
        {
            let track = self.track.read().expect("Read track.");
            action(&track);
        }
        self.update();
    }

    pub fn update_track<Action: FnOnce(&mut Track)>(&mut self, action: Action) {
        {
            let mut track = self.track.write().expect("Write to track.");
            action(&mut track);
        }
        self.update();
    }

    /// Normally should not be used from outside. Made it pub as double-borrow workaround.
    pub fn update(&mut self) {
        let track = self.track.clone();
        let track = track.read().expect("Read track.");
        if track.version != self.track_version {
            self.push();
            // It is possible co keep these, but that would complicate implementation, and UI.
            self.discard_tail();
        }
    }

    // version_diff < 0 for undo. version_diff == 1 - save new version.
    pub fn shift_version(&mut self, version_diff: VersionId) -> Option<VersionId> {
        self.version
            .checked_add(version_diff)
            .filter(|v| *v >= 0)
            .and_then(|v| {
                self.version = v;
                Some(v)
            })
    }

    pub fn go_to(&mut self, version: VersionId) -> bool {
        let track = self.track.clone();
        if let Some(v) = self.list_revisions().find(|v| v.id == version) {
            let mut track = track.write().expect("Read track.");
            track.load_from(&v.snapshot_path);
            self.track_version = track.version;
            true
        } else {
            false
        }
    }

    /// Save current version into history.
    pub fn push(&mut self) {
        self.shift_version(1).unwrap();
        let track = self.track.clone();
        let track = track.read().expect("Read track.");
        track.save_to(&self.current_snapshot_path());
        self.track_version = track.version;
    }

    /// Restore track  from the last saved version.
    pub fn undo(&mut self) {
        if self.shift_version(-1).is_some() {
            if !self.go_to(self.version) {
                self.shift_version(1);
            }
        }
    }

    pub fn redo(&mut self) {
        if self.shift_version(1).is_some() {
            if !self.go_to(self.version) {
                self.shift_version(-1);
            }
        }
    }

    fn discard_tail(&mut self) {
        for v in self.list_revisions() {
            if self.version < v.id {
                fs::remove_file(v.snapshot_path).expect("Delete snapshot.");
            }
        }
    }

    pub fn version(&self) -> VersionId {
        self.version
    }

    fn check_directory_writable(directory: &PathBuf) {
        let metadata = fs::metadata(&directory).expect(
            format!(
                "Cannot init history: {} is not found.",
                &directory.to_string_lossy()
            )
            .as_str(),
        );
        if !metadata.is_dir() {
            panic!(
                "Cannot init history: {} is not a directory.",
                &directory.to_string_lossy()
            );
        }
        if metadata.permissions().readonly() {
            panic!(
                "Cannot init history: {} is not writable.",
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
            track_version: -1,
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
                "Not creating initial version: project history is not empty, '{}' exists.",
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
                self.version = v.id;
                let file_path = self.current_snapshot_path();
                self.update_track(|track| track.load_from(&file_path));
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
                        snapshot_path: p.to_owned(),
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
        let file = file.to_owned();
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
        path.push(format!("{}.{}", version, Self::SNAPSHOT_NAME_EXT));
        path
    }

    pub fn current_snapshot_path(&self) -> PathBuf {
        self.make_snapshot_path(self.version)
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
        assert_eq!(
            Some(145),
            TrackHistory::parse_snapshot_name(&PathBuf::from("145.emmrev.mid"))
        );
    }
}

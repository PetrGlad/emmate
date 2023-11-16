use eframe::egui::ahash::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use std::{fs, mem};

use glob::glob;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::common::VersionId;
use crate::edit_actions::Changeset;
use crate::track::Track;

pub type ActionId = Option<&'static str>;

#[derive(Debug)]
struct ActionThrottle {
    timestamp: Instant,
    action_id: ActionId,
    changeset: Changeset,
}

impl ActionThrottle {
    pub fn is_waiting(&self, now: Instant) -> bool {
        now - self.timestamp < Duration::from_millis(400)
    }
}

// Undo/redo history and snapshots.
#[derive(Debug)]
pub struct TrackHistory {
    /// Normally should not be used from outside. Made it pub as double-borrow workaround.
    pub track: Arc<RwLock<Track>>,
    track_version: VersionId,
    pub version: VersionId,
    pub directory: PathBuf,
    throttle: Option<ActionThrottle>,
    // TODO Changesets should be persisted. Keeping in memory as a prototype implementation.
    changesets: HashMap<VersionId, Changeset>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    id: VersionId,
    snapshot_path: PathBuf,
}

impl TrackHistory {
    const SNAPSHOT_NAME_EXT: &'static str = "emmrev.mid";

    pub fn with_track<Out, Action: FnOnce(&Track) -> Out>(&self, action: Action) -> Out {
        let track = self.track.read().expect("Read track.");
        action(&track)
    }

    pub fn update_track<Action: FnOnce(&mut Track, &mut Changeset)>(
        &mut self,
        action_id: ActionId,
        action: Action,
    ) {
        let changeset = {
            let mut track = self.track.write().expect("Write to track.");
            let mut changeset = Changeset::default();
            action(&mut track, &mut changeset);
            changeset
        };

        let now = Instant::now();
        if let Some(throttle) = &mut self.throttle {
            if throttle.action_id == action_id && throttle.is_waiting(now) {
                throttle.changeset.merge(changeset);
                return;
            }
            let throttle = mem::replace(&mut self.throttle, None).unwrap();
            self.update(throttle.changeset);
        } else {
            self.throttle = Some(ActionThrottle {
                timestamp: now,
                action_id,
                changeset,
            });
        }
    }

    pub fn do_pending(&mut self) {
        // This is ugly, just making it work for now. History may need some scheduled events.
        // To do this asynchronously one would need to put id behind an Arc<RwLock<>> or
        // use Tokio here (and in the engine).
        if let Some(throttle) = &self.throttle {
            if !throttle.is_waiting(Instant::now()) {
                let throttle = mem::replace(&mut self.throttle, None).unwrap();
                self.update(throttle.changeset);
                self.throttle = None;
            }
        }
    }

    /// Normally should not be used from outside. Made it pub as double-borrow workaround.
    pub fn update(&mut self, changeset: Changeset) {
        let track_version = self.with_track(|t| t.version);
        if track_version != self.track_version {
            self.push(changeset);
            self.discard_tail();
            self.track_version = track_version;
        }
    }

    // version_diff < 0 for undo, version_diff > 0 to the next version or redo.
    pub fn shift_version(&mut self, version_diff: VersionId) -> Option<VersionId> {
        let v = self.version + version_diff;
        if v >= 0 {
            self.version = v;
            Some(v)
        } else {
            None
        }
    }

    pub fn go_to_version(&mut self) -> bool {
        if let Some(v) = self.get_version(self.version) {
            let track = self.track.clone();
            let mut track = track.write().expect("Read track.");
            track.load_from(&v.snapshot_path);
            self.track_version = track.version;
            self.write_meta();
            true
        } else {
            false
        }
    }

    /// Save current version into history.
    pub fn push(&mut self, changeset: Changeset) {
        self.shift_version(1).unwrap();
        self.changesets.insert(self.version, changeset);
        let track = self.track.clone();
        let track = track.read().expect("Read track.");
        track.save_to(&self.current_snapshot_path());
        self.track_version = track.version;
        self.write_meta();
    }

    /// Restore track from the last saved version.
    pub fn undo(&mut self) {
        if self.shift_version(-1).is_some() {
            if !self.go_to_version() {
                self.shift_version(1);
            }
        }
    }

    pub fn redo(&mut self) {
        if self.shift_version(1).is_some() {
            if !self.go_to_version() {
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
        Self {
            directory: directory.to_owned(),
            version: 0,
            track: Default::default(),
            track_version: -1,
            throttle: None,
            changesets: HashMap::default(),
        }
    }

    /// Create the fist version of a new history.
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
        self.write_meta();
        self
    }

    pub fn open(&mut self) {
        Self::check_directory_writable(&self.directory);
        dbg!(self.list_revisions().collect::<Vec<Version>>());
        if let Some(meta) = self.load_meta() {
            self.version = meta.current_version;
            let file_path = self.current_snapshot_path();
            let mut track = self.track.write().expect("Write to track.");
            track.load_from(&file_path);
        } else {
            panic!("Cannot load revision history metadata.");
        }
    }

    fn write_meta(&self) {
        let mut binary = Vec::new();
        let meta = Meta {
            current_version: self.version,
        };
        dbg!(&meta);
        meta.serialize(&mut rmp_serde::Serializer::new(&mut binary).with_struct_map())
            .expect("serialize history meta");
        fs::write(self.make_meta_path(), binary).expect("store history meta");
    }

    fn load_meta(&self) -> Option<Meta> {
        if let Ok(binary) = fs::read(self.make_meta_path()) {
            let meta = rmp_serde::from_slice(&binary).expect("deserialize history meta");
            dbg!(&meta);
            Some(meta)
        } else {
            None
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

    fn get_version(&self, version_id: VersionId) -> Option<Version> {
        let path = self.make_snapshot_path(version_id);
        if path.is_file() {
            Some(Version {
                id: version_id,
                snapshot_path: path,
            })
        } else {
            None
        }
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

    fn make_meta_path(&self) -> PathBuf {
        let mut path = self.directory.clone();
        path.push("meta.mpack");
        path
    }

    pub fn current_snapshot_path(&self) -> PathBuf {
        self.make_snapshot_path(self.version)
    }
}

/// Additional history data that should be persisted.
#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    current_version: VersionId,
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

    #[test]
    fn snapshot_path() {
        let history = TrackHistory::with_directory(&PathBuf::from("."));
        assert_eq!(
            TrackHistory::parse_snapshot_name(&history.make_snapshot_path(123)),
            Some(123)
        );
    }

    #[test]
    fn meta_serialization() {
        let mut history = TrackHistory::with_directory(&PathBuf::from("target"));
        history.version = 321;
        history.write_meta();
        history.version = 12;
        let m = history.load_meta();
        assert_eq!(321, m.unwrap().current_version);
    }
}

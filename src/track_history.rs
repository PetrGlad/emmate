use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use std::{fs, mem};

use glob::glob;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::changeset::{Changeset, Patch, Snapshot};
use crate::common::VersionId;
use crate::track::Track;
use crate::util;
use crate::util::IdSeq;

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
    pub version: VersionId,
    pub directory: PathBuf,
    throttle: Option<ActionThrottle>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    id: VersionId,
    snapshot_path: Option<PathBuf>,
    diff_path: Option<PathBuf>,
}

impl Version {
    pub fn is_empty(&self) -> bool {
        self.snapshot_path.is_none() && self.diff_path.is_none()
    }
}

impl TrackHistory {
    const SNAPSHOT_NAME_EXT: &'static str = "snapshot";
    const DIFF_NAME_EXT: &'static str = "changeset";

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
            let mut changeset = Changeset::empty();
            action(&mut track, &mut changeset);
            track.patch(&changeset);
            track.commit();
            changeset
        };

        // TODO Reimplement throttling (batch sequences of repeatable
        //   heavy operations like tail_shift).
        dbg!(action_id, changeset.changes.len());
        self.update(changeset);
        // let now = Instant::now();
        // if let Some(throttle) = &mut self.throttle {
        //     if action_id.is_some() && throttle.action_id == action_id && throttle.is_waiting(now) {
        //         throttle.changeset.merge(changeset);
        //         return;
        //     }
        //     let throttle = mem::replace(&mut self.throttle, None).unwrap();
        //     self.update(throttle.changeset);
        //     self.throttle = None;
        // } else {
        //     self.throttle = Some(ActionThrottle {
        //         timestamp: now,
        //         action_id,
        //         changeset,
        //     });
        // }
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
    fn update(&mut self, changeset: Changeset) {
        if changeset.changes.is_empty() {
            return;
        }
        // The changeset should be already applied to track by now.
        self.push(changeset);
        self.discard_tail();
    }

    pub fn is_valid_version_id(v: VersionId) -> bool {
        v >= 0
    }

    pub fn go_to_version(&mut self, version_id: VersionId) -> bool {
        assert!(TrackHistory::is_valid_version_id(version_id));
        let version = self.get_version(version_id);
        assert_eq!(version.id, version_id);
        if version.is_empty() {
            return false;
        }
        let track = self.track.clone();
        let mut track = track.write().expect("Read track.");
        // TODO Support multiple snapshots. Now expecting only a starting one with id 0.
        //   Should use snapshot if it is found but diff is missing.
        //   Maybe prefer snapshots when both diff and snapshot are present.
        if let Some(snapshot_path) = version.snapshot_path {
            track.reset(util::load(&snapshot_path));
            self.version = version.id;
            return true;
        }
        // Replays
        while self.version < version_id {
            let diff: Patch = util::load(&self.diff_path(self.version + 1));
            assert_eq!(diff.base_version, self.version);
            assert!(diff.version > self.version);
            let mut cs = Changeset::empty();
            cs.add_all(&diff.changes);
            track.patch(&cs);
            self.version = diff.version;
        }
        // Rollbacks
        while self.version > version_id {
            let diff: Patch = util::load(&self.diff_path(self.version));
            assert_eq!(diff.version, self.version);
            assert!(diff.base_version < self.version);
            let mut cs = Changeset::empty();
            cs.add_all(&diff.changes);
            track.revert(&cs);
            self.version = diff.base_version;
        }
        dbg!(self.version, version_id);
        self.version == version_id
    }

    /// Save current version into history.
    pub fn push(&mut self, changeset: Changeset) {
        let diff = Patch {
            base_version: self.version,
            version: self.version + 1,
            changes: changeset.changes.values().cloned().collect(),
        };
        util::store(&diff, &self.diff_path(diff.version));
        // TODO also store a new snapshot here if necessary
        //   (avoid long timeouts and long diff-only runs between snapshots).
        self.version = diff.version;
        self.write_meta();
    }

    /// Maybe undo last edit action.
    pub fn undo(&mut self) {
        let prev_version_id = self.version - 1;
        if TrackHistory::is_valid_version_id(prev_version_id) {
            assert!(self.go_to_version(prev_version_id));
        }
    }

    /// Maybe redo next edit action.
    pub fn redo(&mut self) {
        self.go_to_version(self.version + 1);
    }

    fn discard_tail(&mut self) {
        let mut version_id = self.version;
        loop {
            version_id += 1;
            let version = self.get_version(version_id);
            if version.is_empty() {
                break;
            }
            if let Some(path) = version.snapshot_path {
                fs::remove_file(path).expect("delete snapshot");
            }
            if let Some(path) = version.diff_path {
                fs::remove_file(path).expect("delete diff");
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
            throttle: None,
        }
    }

    /// Create the fist version of a new history.
    pub fn init(mut self, source_file: &PathBuf) -> Self {
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
        let version = self.version;
        self.update_track(None, |track, _changeset| {
            track.import_smf(source_file);
            util::store(&Snapshot::of_track(version, track), &starting_snapshot_path);
        });
        self.write_meta();
        self
    }

    pub fn open(&mut self) {
        Self::check_directory_writable(&self.directory);
        let meta = self.load_meta();
        let initial_version_id = 0;
        {
            let mut track = self.track.write().expect("Write to track.");
            track.id_seq = IdSeq::new(meta.next_id);
            track.reset(util::load(&self.snapshot_path(initial_version_id)));
        }
        self.version = initial_version_id;
        assert!(self.go_to_version(meta.current_version));
    }

    fn write_meta(&self) {
        let meta = Meta {
            next_id: self.with_track(|t| t.id_seq.current()),
            current_version: self.version,
        };
        dbg!(&meta);
        util::store(&meta, &self.make_meta_path());
    }

    fn load_meta(&self) -> Meta {
        util::load(&self.make_meta_path())
    }

    fn list_snapshots(&self) -> impl Iterator<Item = (VersionId, PathBuf)> {
        let mut pattern = self.directory.to_owned();
        pattern.push("*.".to_string() + Self::SNAPSHOT_NAME_EXT);
        let files = glob(pattern.to_str().unwrap()).expect("List snapshots directory.");
        files
            .flatten() // XXX `flatten` may hide metadata errors.
            .map(|p| {
                assert!(p.is_file());
                if let Some(id) = Self::parse_snapshot_name(&p) {
                    Some((id, p.to_owned()))
                } else {
                    None
                }
            })
            .flatten()
    }

    fn get_version(&self, version_id: VersionId) -> Version {
        let diff_path = self.diff_path(version_id);
        let snapshot_path = self.snapshot_path(version_id);
        Version {
            id: version_id,
            snapshot_path: if snapshot_path.is_file() {
                Some(snapshot_path)
            } else {
                None
            },
            diff_path: if diff_path.is_file() {
                Some(diff_path)
            } else {
                None
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.get_version(0).is_empty()
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

    fn snapshot_path(&self, version: VersionId) -> PathBuf {
        let mut path = self.directory.clone();
        path.push(format!("{}.{}", version, Self::SNAPSHOT_NAME_EXT));
        path
    }

    fn diff_path(&self, version: VersionId) -> PathBuf {
        let mut path = self.directory.clone();
        path.push(format!("{}.{}", version, Self::DIFF_NAME_EXT));
        path
    }

    fn make_meta_path(&self) -> PathBuf {
        let mut path = self.directory.clone();
        path.push("meta");
        path
    }

    pub fn current_snapshot_path(&self) -> PathBuf {
        self.snapshot_path(self.version)
    }
}

/// Additional history data that should be persisted.
#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    next_id: u64,
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
            TrackHistory::parse_snapshot_name(&PathBuf::from("5.snapshot"))
        );
        assert_eq!(
            Some(145),
            TrackHistory::parse_snapshot_name(&PathBuf::from("145.snapshot"))
        );
    }

    #[test]
    fn snapshot_path() {
        let history = TrackHistory::with_directory(&PathBuf::from("."));
        assert_eq!(
            TrackHistory::parse_snapshot_name(&history.snapshot_path(123)),
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
        assert_eq!(321, m.current_version);
        assert_eq!(0, m.next_id);
    }
}

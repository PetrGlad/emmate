use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use glob::glob;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sync_cow::SyncCow;

use crate::changeset::{EventAction, EventActionsList, HistoryLogEntry, Snapshot};
use crate::common::VersionId;
use crate::track::{import_smf, Track};
use crate::track_edit::{apply_diffs, revert_diffs, AppliedCommand, CommandDiff, EditCommandId};
use crate::util;
use crate::util::IdSeq;

// Undo/redo history and snapshots.
// #[derive(Debug)] // Debug is not implemented for SyncCow
pub struct TrackHistory {
    pub track: Arc<SyncCow<Track>>,
    pub id_seq: Arc<IdSeq>,
    version: VersionId,
    pub max_version: VersionId, // May be higher than self.version after an undo.
    pub directory: PathBuf,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    id: VersionId,
    snapshot_path: Option<PathBuf>,
    diff_path: Option<PathBuf>,
}

pub type CommandApplication = Option<(AppliedCommand, EventActionsList)>;

impl Version {
    pub fn is_empty(&self) -> bool {
        self.snapshot_path.is_none() && self.diff_path.is_none()
    }
}

impl TrackHistory {
    const SNAPSHOT_NAME_EXT: &'static str = "ss";
    const DIFF_NAME_EXT: &'static str = "d";

    pub fn with_track<Out, Action: FnOnce(&Track) -> Out>(&self, action: Action) -> Out {
        let track = self.track.read();
        action(&track)
    }

    pub fn update_track<Action: FnOnce(&Track) -> Option<AppliedCommand>>(
        &mut self,
        action: Action,
    ) -> CommandApplication {
        let applied_command = {
            let track = self.track.read();
            action(&track)
        };
        if let Some(applied_command) = applied_command {
            let mut changes = vec![];
            self.track.edit(|track: &mut Track| {
                apply_diffs(track, &applied_command.1, &mut changes);
                track.commit();
            });
            self.update(&applied_command);
            Some((applied_command, changes))
        } else {
            None
        }
    }

    fn update(&mut self, applied_command: &(EditCommandId, Vec<CommandDiff>)) {
        let (command_id, diff) = applied_command;
        if diff.is_empty() {
            dbg!("No changes.");
            return;
        }
        // The changeset should be already applied to track by now.
        let log_entry = HistoryLogEntry {
            base_version: self.version,
            version: self.version + 1,
            command_id: *command_id,
            diff: diff.iter().cloned().collect(), // XXX Maybe share the vector?
        };
        self.push(log_entry);
        // TODO also store a new snapshot here if necessary
        //   (to avoid long timeouts and long diff-only runs between snapshots).
        self.discard_tail(self.max_version);
    }

    /// Save current version into history.
    pub fn push(&mut self, log_entry: HistoryLogEntry) {
        util::store(&log_entry, &self.diff_path(log_entry.version));
        self.set_version(log_entry.version);
        self.max_version = self.version;
        self.write_meta();
    }

    pub fn is_valid_version_id(v: VersionId) -> bool {
        v >= 0
    }

    /// Returns true if the required version's state was restored successfully.
    /// If the exact version number is not found, apply as many patches as possible to get close to it.
    pub fn go_to_version(&mut self, version_id: VersionId, changes: &mut EventActionsList) -> bool {
        // TODO (optimization) A streak of multiple actions may temporarily accumulate many events
        //   in `changes`. This will likely happen at startup. It is possible compact them into
        //   a changeset immediately, but want to profile both options before deciding.
        assert!(TrackHistory::is_valid_version_id(version_id));
        let version = self.get_version(version_id);
        assert_eq!(version.id, version_id);
        if version.is_empty() {
            println!("No track history found.");
            return false;
        }
        {
            let track = self.track.clone();
            track.edit(|track| self.apply_patches(changes, version, track));
        }
        dbg!(self.version, version_id);
        self.write_meta();
        self.version == version_id
    }

    /// Return true if exact snapshot is found.
    fn apply_patches(
        &mut self,
        changes: &mut EventActionsList,
        version: Version,
        mut track: &mut Track,
    ) {
        // TODO (feature, persistence) Support multiple snapshots. Now expecting only the starting one with id 0.
        //   Should use snapshot if it is found but diff is missing.
        //   Maybe prefer snapshots when both diff and snapshot are present.
        if let Some(snapshot_path) = version.snapshot_path {
            track.reset(util::load(&snapshot_path));
            self.set_version(version.id);
            println!("Found a snapshot for revision {}.", version.id);
        }
        // Replays
        while self.version < version.id {
            let entry: HistoryLogEntry = util::load(&self.diff_path(self.version + 1));
            assert_eq!(entry.base_version, self.version);
            assert!(entry.version > self.version);
            apply_diffs(&mut track, &entry.diff, changes);
            self.set_version(entry.version);
        }
        // Rollbacks
        while self.version > version.id {
            let entry: HistoryLogEntry = util::load(&self.diff_path(self.version));
            assert_eq!(entry.version, self.version);
            assert!(entry.base_version < self.version);
            revert_diffs(&mut track, &entry.diff, changes);
            self.set_version(entry.base_version);
        }
    }

    /** Maybe undo last edit action.
       `changes` parameter may be used to accumulate a series of patches for persistence.
    */
    pub fn undo(&mut self, changes: &mut EventActionsList) -> bool {
        // FIXME (new-events) Notify stave of changes in history (it needs to refresh cached data now).
        //   Stave may listen to some notification channel or look at the track version.
        let prev_version_id = self.version - 1;
        if TrackHistory::is_valid_version_id(prev_version_id) {
            assert!(self.go_to_version(prev_version_id, changes));
            true
        } else {
            false
        }
    }

    /// Maybe redo next edit action.
    pub fn redo(&mut self, changes: &mut EventActionsList) -> bool {
        self.go_to_version(self.version + 1, changes)
    }

    fn discard_tail(&mut self, max_version: VersionId) {
        // Note that  in some cases (e.g. program termination) this procedure may not complete,
        // leaving some of the files in place.
        let mut version_id = max_version;
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
            id_seq: Arc::new(IdSeq::new(0)),
            version: 0,
            max_version: 0,
            track: Arc::new(SyncCow::new(Track::default())),
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

        {
            let id_seq = self.id_seq.clone();
            self.update_track(|track| {
                let mut patch = vec![];
                for ev in import_smf(&id_seq, source_file) {
                    patch.push(EventAction::Insert(ev));
                }
                util::store(&Snapshot::of_track(version, track), &starting_snapshot_path);
                Some((EditCommandId::Load, vec![CommandDiff::ChangeList { patch }]))
            });
        }
        self.write_meta();
        self
    }

    pub fn open(&mut self) {
        Self::check_directory_writable(&self.directory);
        let meta = self.load_meta();
        let initial_version_id = 0;
        {
            self.id_seq = Arc::new(IdSeq::new(meta.next_id));
            self.track
                .edit(|track| track.reset(util::load(&self.snapshot_path(initial_version_id))));
        }
        self.set_version(initial_version_id);
        assert!(self.go_to_version(meta.current_version, &mut vec![]));
    }

    fn set_version(&mut self, version_id: VersionId) {
        assert!(TrackHistory::is_valid_version_id(version_id));
        assert!(TrackHistory::is_valid_version_id(self.max_version));
        self.version = version_id;
        if self.max_version < self.version {
            self.max_version = self.version
        }
    }

    fn write_meta(&self) {
        let meta = Meta {
            next_id: self.id_seq.current(),
            current_version: self.version,
            max_version: self.max_version,
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
    max_version: VersionId,
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
        history.set_version(321);
        history.write_meta();
        history.set_version(12);
        let m = history.load_meta();
        assert_eq!(321, m.current_version);
        assert_eq!(0, m.next_id);
    }
}

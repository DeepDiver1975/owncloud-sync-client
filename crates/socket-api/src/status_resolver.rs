use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use camino::Utf8PathBuf;
use uuid::Uuid;

use sync_engine::state::{FileStatus, SyncState};

pub struct StatusResolver {
    sync_states: Arc<RwLock<HashMap<Uuid, SyncState>>>,
    folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>>,
}

impl StatusResolver {
    pub fn new(
        sync_states: Arc<RwLock<HashMap<Uuid, SyncState>>>,
        folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>>,
    ) -> Self {
        Self {
            sync_states,
            folder_roots,
        }
    }

    pub fn resolve_file(&self, path: &str) -> &'static str {
        let Some(folder_id) = self.find_folder_for_path(path) else {
            return "NONE";
        };

        let states = self.sync_states.read().unwrap();
        let Some(state) = states.get(&folder_id) else {
            return "NONE";
        };

        let utf8_path = Utf8PathBuf::from(path);
        match state.file_statuses.get(&utf8_path) {
            None => "OK",
            Some(FileStatus::Ok) => "OK",
            Some(FileStatus::Syncing) => "SYNC",
            Some(FileStatus::Error(_)) => "ERROR",
            Some(FileStatus::Excluded) => "EXCLUDED",
        }
    }

    pub fn resolve_folder(&self, path: &str) -> &'static str {
        let Some(folder_id) = self.find_folder_for_path(path) else {
            return "NONE";
        };

        let states = self.sync_states.read().unwrap();
        let Some(state) = states.get(&folder_id) else {
            return "NONE";
        };

        let prefix = Utf8PathBuf::from(path);
        let mut worst: &'static str = "OK";

        for (file_path, status) in &state.file_statuses {
            if !file_path.starts_with(&prefix) {
                continue;
            }
            let tag = match status {
                FileStatus::Ok => "OK",
                FileStatus::Syncing => "SYNC",
                FileStatus::Error(_) => "ERROR",
                FileStatus::Excluded => "EXCLUDED",
            };
            worst = worse_status(worst, tag);
        }

        worst
    }

    pub fn find_folder_for_path(&self, path: &str) -> Option<Uuid> {
        let roots = self.folder_roots.read().unwrap();
        let path_buf = Utf8PathBuf::from(path);
        roots
            .iter()
            .filter(|(root, _)| path_buf.starts_with(root))
            .max_by_key(|(root, _)| root.as_str().len())
            .map(|(_, id)| *id)
    }
}

fn worse_status(a: &'static str, b: &'static str) -> &'static str {
    fn priority(s: &str) -> u8 {
        match s {
            "ERROR" => 3,
            "SYNC" => 2,
            "EXCLUDED" => 1,
            _ => 0,
        }
    }
    if priority(b) > priority(a) {
        b
    } else {
        a
    }
}

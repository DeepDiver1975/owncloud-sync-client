use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use uuid::Uuid;

use crate::error::SyncError;

/// High-level status of a watched folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderStatus {
    Idle,
    Syncing,
    Error,
}

/// Per-file status exposed to the UI layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileStatus {
    Ok,
    Syncing,
    Error(String),
    Excluded,
}

/// All mutable state for one sync folder, typically held behind `Arc<RwLock<>>`.
#[derive(Debug)]
pub struct SyncState {
    pub folder_id: Uuid,
    pub status: FolderStatus,
    pub file_statuses: HashMap<Utf8PathBuf, FileStatus>,
    pub last_sync: Option<SystemTime>,
    pub errors: Vec<SyncError>,
}

impl SyncState {
    pub fn new(folder_id: Uuid) -> Self {
        Self {
            folder_id,
            status: FolderStatus::Idle,
            file_statuses: HashMap::new(),
            last_sync: None,
            errors: Vec::new(),
        }
    }

    pub fn set_file_status(&mut self, path: Utf8PathBuf, status: FileStatus) {
        self.file_statuses.insert(path, status);
    }

    pub fn record_error(&mut self, error: SyncError) {
        self.status = FolderStatus::Error;
        self.errors.push(error);
    }

    pub fn mark_complete(&mut self) {
        self.status = FolderStatus::Idle;
        self.last_sync = Some(SystemTime::now());
    }
}

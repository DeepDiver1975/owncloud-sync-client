use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use crate::error::SyncError;

#[derive(Debug, Clone)]
pub struct LocalEntry {
    pub path: Utf8PathBuf,
    pub mtime: SystemTime,
    pub size: u64,
    pub inode: u64,
    pub is_virtual: bool,
}

#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub path: Utf8PathBuf,
    pub etag: String,
    pub mtime: SystemTime,
    pub size: u64,
    pub file_id: String,
    pub permissions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncInstruction {
    Upload,
    Download,
    DeleteLocal,
    DeleteRemote,
    RenameLocal { to: Utf8PathBuf },
    RenameRemote { to: Utf8PathBuf },
    Conflict,
    UpdateMetadata,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictStrategy {
    KeepBoth,
    KeepRemote,
    KeepLocal,
}

/// A single file's sync work item. Does not derive Clone because `SyncError`
/// contains `std::io::Error`, which is not Clone.
#[derive(Debug)]
pub struct SyncFileItem {
    pub path: Utf8PathBuf,
    pub instruction: SyncInstruction,
    pub direction: Direction,
    pub etag: Option<String>,
    pub size: u64,
    pub mtime: SystemTime,
    pub file_id: Option<String>,
    pub checksum: Option<String>,
    pub error: Option<SyncError>,
}

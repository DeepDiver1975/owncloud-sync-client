// crates/sync-db/src/models.rs
use serde::{Deserialize, Serialize};

/// Represents a row in the `metadata` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct JournalEntry {
    pub path: String,
    pub etag: Option<String>,
    /// Unix timestamp (seconds since epoch).
    pub mtime: Option<i64>,
    pub size: Option<i64>,
    pub inode: Option<i64>,
    pub file_id: Option<String>,
    pub checksum: Option<String>,
    /// Non-zero if this is a virtual (placeholder) entry.
    pub is_virtual: i64,
}

impl JournalEntry {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            etag: None,
            mtime: None,
            size: None,
            inode: None,
            file_id: None,
            checksum: None,
            is_virtual: 0,
        }
    }

    pub fn is_virtual_file(&self) -> bool {
        self.is_virtual != 0
    }
}

/// Represents a row in the `upload_info` table (in-progress TUS upload state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct UploadInfo {
    pub path: String,
    pub upload_id: String,
    pub offset: i64,
    pub size: i64,
}

impl UploadInfo {
    pub fn new(path: impl Into<String>, upload_id: impl Into<String>, size: i64) -> Self {
        Self {
            path: path.into(),
            upload_id: upload_id.into(),
            offset: 0,
            size,
        }
    }
}

/// Represents a row in the `error_blacklist` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ErrorBlacklistEntry {
    pub path: String,
    pub error_count: i64,
    pub last_error: String,
    /// Unix timestamp: do not retry before this time.
    pub retry_after: i64,
}

impl ErrorBlacklistEntry {
    pub fn new(
        path: impl Into<String>,
        error_count: i64,
        last_error: impl Into<String>,
        retry_after: i64,
    ) -> Self {
        Self {
            path: path.into(),
            error_count,
            last_error: last_error.into(),
            retry_after,
        }
    }
}

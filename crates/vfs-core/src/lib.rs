use async_trait::async_trait;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Metadata the sync engine passes when creating a placeholder file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsFileItem {
    pub path: Utf8PathBuf,
    pub size: u64,
    pub etag: String,
    pub file_id: String,
    /// Last-modified time used to stamp placeholder metadata.
    #[serde(with = "serde_millis")]
    pub last_modified: std::time::SystemTime,
}

/// Hydration state of a VFS entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VfsStatus {
    /// File is fully present on disk.
    Full,
    /// Placeholder exists; content has not been downloaded.
    Placeholder,
    /// File is being hydrated (partial download in progress).
    Syncing,
}

/// Errors produced by VFS operations.
#[derive(Debug, Error)]
pub enum VfsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("VFS operation not supported on this platform")]
    NotSupported,

    #[error("Path not found: {path}")]
    NotFound { path: Utf8PathBuf },

    #[error("VFS backend error: {0}")]
    Backend(String),
}

/// Abstraction over OS-level virtual filesystem support.
///
/// Implementations must be `Send + Sync` so they can be shared across tasks.
#[async_trait]
pub trait Vfs: Send + Sync {
    /// Create a dehydrated placeholder for `item`.
    /// The `item.path` is relative to the sync root.
    async fn create_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError>;

    /// Update metadata of an existing placeholder.
    async fn update_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError>;

    /// Force-hydrate the file at `path`.
    async fn hydrate(&self, path: &Utf8Path) -> Result<(), VfsError>;

    /// Dehydrate the file at `path` back to a placeholder.
    async fn dehydrate(&self, path: &Utf8Path) -> Result<(), VfsError>;

    /// Return `true` if the file at `path` is a virtual placeholder.
    async fn is_virtual(&self, path: &Utf8Path) -> Result<bool, VfsError>;

    /// Return the current [`VfsStatus`] of `path`.
    async fn status(&self, path: &Utf8Path) -> Result<VfsStatus, VfsError>;

    /// Pin or unpin `path`.
    async fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<(), VfsError>;
}

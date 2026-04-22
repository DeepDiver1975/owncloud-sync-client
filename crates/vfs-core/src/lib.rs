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
    /// Create a placeholder (dehydrated) entry at `path`.
    async fn create_placeholder(
        &self,
        path: &Utf8Path,
        item: &VfsFileItem,
    ) -> Result<(), VfsError>;

    /// Trigger on-demand hydration of a placeholder.
    async fn hydrate(&self, path: &Utf8Path) -> Result<(), VfsError>;

    /// Convert a full file back into a placeholder to free disk space.
    async fn dehydrate(&self, path: &Utf8Path) -> Result<(), VfsError>;

    /// Return the current [`VfsStatus`] of `path`.
    async fn status(&self, path: &Utf8Path) -> Result<VfsStatus, VfsError>;

    /// Pin or unpin `path` (pinned files are never automatically dehydrated).
    async fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<(), VfsError>;
}

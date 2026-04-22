use camino::Utf8PathBuf;
use thiserror::Error;

/// All errors that can occur inside the sync engine.
#[derive(Debug, Error)]
pub enum SyncError {
    /// An HTTP operation returned an unexpected status code.
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },

    /// A filesystem I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A database operation failed.
    #[error("Database error: {0}")]
    Db(String),

    /// A VFS operation failed.
    #[error("VFS error: {0}")]
    Vfs(String),

    /// Failed to parse a value (XML, JSON, header, ...).
    #[error("Parse error: {0}")]
    Parse(String),

    /// Two versions of a file conflict and automatic resolution was not possible.
    #[error("Conflict at path: {path}")]
    Conflict { path: Utf8PathBuf },

    /// The sync was cancelled externally.
    #[error("Sync cancelled")]
    Cancelled,
}

impl From<vfs_core::VfsError> for SyncError {
    fn from(e: vfs_core::VfsError) -> Self {
        SyncError::Vfs(e.to_string())
    }
}

/// Convenience alias.
pub type Result<T, E = SyncError> = std::result::Result<T, E>;

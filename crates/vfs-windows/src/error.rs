use camino::Utf8PathBuf;
use thiserror::Error;
use vfs_core::VfsError;

#[derive(Debug, Error)]
pub enum VfsWindowsError {
    #[cfg(target_os = "windows")]
    #[error("CfAPI error: {0}")]
    CfApi(#[from] windows::core::Error),

    #[error("Path not found: {0}")]
    PathNotFound(Utf8PathBuf),

    #[error("Operation not supported: {0}")]
    NotSupported(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("String conversion error: {0}")]
    StringConversion(String),

    #[error("VFS backend error: {0}")]
    Backend(String),
}

impl From<VfsWindowsError> for VfsError {
    fn from(e: VfsWindowsError) -> Self {
        match e {
            VfsWindowsError::PathNotFound(path) => VfsError::NotFound { path },
            VfsWindowsError::NotSupported(_) => VfsError::NotSupported,
            VfsWindowsError::Io(io) => VfsError::Io(io),
            other => VfsError::Backend(other.to_string()),
        }
    }
}

pub type Result<T, E = VfsWindowsError> = std::result::Result<T, E>;

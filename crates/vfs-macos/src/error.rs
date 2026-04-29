use thiserror::Error;
use vfs_core::VfsError;

#[derive(Debug, Error)]
pub enum VfsMacOsError {
    #[error("XPC error: {0}")]
    Xpc(String),
    #[error("Protocol error: {0}")]
    Protocol(String),
}

impl From<VfsMacOsError> for VfsError {
    fn from(e: VfsMacOsError) -> Self {
        VfsError::Backend(e.to_string())
    }
}

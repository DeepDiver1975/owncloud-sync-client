use thiserror::Error;

#[derive(Debug, Error)]
pub enum SocketApiError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("VFS error: {0}")]
    Vfs(#[from] vfs_core::VfsError),
}

pub type Result<T, E = SocketApiError> = std::result::Result<T, E>;

use std::path::{Path, PathBuf};

use tokio::net::UnixListener;

use crate::error::{Result, SocketApiError};
use crate::transport::{Connection, Transport};

pub struct UnixTransport {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl UnixTransport {
    pub async fn bind(path: &Path) -> Result<Self> {
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| {
                SocketApiError::Transport(format!(
                    "failed to remove stale socket {}: {e}",
                    path.display()
                ))
            })?;
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                SocketApiError::Transport(format!(
                    "failed to create socket directory {}: {e}",
                    parent.display()
                ))
            })?;
        }

        let listener = UnixListener::bind(path).map_err(|e| {
            SocketApiError::Transport(format!(
                "failed to bind Unix socket {}: {e}",
                path.display()
            ))
        })?;

        Ok(Self {
            listener,
            socket_path: path.to_owned(),
        })
    }

    pub fn default_path() -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home)
                .join("Library/Group Containers/owncloud.socketapi")
                .join("owncloud.sock")
        }
        #[cfg(not(target_os = "macos"))]
        {
            let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(runtime).join("owncloud").join("socket")
        }
    }
}

#[async_trait::async_trait]
impl Transport for UnixTransport {
    async fn accept(&self) -> Result<Connection> {
        let (stream, _addr) = self
            .listener
            .accept()
            .await
            .map_err(|e| SocketApiError::Transport(format!("accept failed: {e}")))?;
        Ok(Box::new(stream))
    }
}

impl Drop for UnixTransport {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

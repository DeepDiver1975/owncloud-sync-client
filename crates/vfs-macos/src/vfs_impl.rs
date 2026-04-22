//! `VfsMacOs` — the public struct implementing `vfs_core::Vfs` on macOS via XPC.

use std::sync::Mutex;
use std::time::SystemTime;

use async_trait::async_trait;
use camino::{Utf8Path, Utf8PathBuf};

use vfs_core::{Vfs, VfsError, VfsFileItem, VfsStatus};

use crate::error::VfsMacOsError;
use crate::messages::{XpcCommand, XpcReply};
use crate::xpc::XpcConnection;

const XPC_SERVICE: &str = "org.owncloud.owncloud-sync.fileprovider-xpc";

/// macOS VFS backend that delegates all operations to the Swift FileProvider
/// extension via an XPC connection.
pub struct VfsMacOs {
    conn: Mutex<XpcConnection>,
    root: Utf8PathBuf,
}

// Safety: XpcConnection is Send + Sync; Mutex<XpcConnection> is Send + Sync.
unsafe impl Send for VfsMacOs {}
unsafe impl Sync for VfsMacOs {}

impl VfsMacOs {
    /// Connect to the FileProvider XPC service and return a new `VfsMacOs`.
    pub fn new(root: Utf8PathBuf) -> Result<Self, VfsMacOsError> {
        let conn = XpcConnection::connect(XPC_SERVICE)?;
        Ok(Self {
            conn: Mutex::new(conn),
            root,
        })
    }

    fn send_cmd(&self, cmd: XpcCommand) -> Result<XpcReply, VfsMacOsError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| VfsMacOsError::Xpc("connection mutex poisoned".to_string()))?;
        conn.send_command(&cmd)
    }

    /// Resolve `abs_path` to a sync-root-relative forward-slash string.
    fn rel_path(&self, abs_path: &Utf8Path) -> String {
        abs_path
            .strip_prefix(&self.root)
            .unwrap_or(abs_path)
            .as_str()
            .to_owned()
    }

    fn expect_ok(reply: XpcReply) -> Result<(), VfsMacOsError> {
        if reply.ok {
            Ok(())
        } else {
            Err(VfsMacOsError::Protocol(
                reply.error.unwrap_or_else(|| "unknown error".to_string()),
            ))
        }
    }

    fn system_time_to_unix(t: SystemTime) -> i64 {
        t.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }
}

#[async_trait]
impl Vfs for VfsMacOs {
    async fn create_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError> {
        let cmd = XpcCommand::CreatePlaceholder {
            path: self.rel_path(&item.path),
            etag: item.etag.clone(),
            size: item.size,
            mtime: Self::system_time_to_unix(item.last_modified),
        };
        let reply = self.send_cmd(cmd).map_err(VfsError::from)?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }

    async fn update_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError> {
        let cmd = XpcCommand::UpdatePlaceholder {
            path: self.rel_path(&item.path),
            etag: item.etag.clone(),
            size: item.size,
            mtime: Self::system_time_to_unix(item.last_modified),
        };
        let reply = self.send_cmd(cmd).map_err(VfsError::from)?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }

    async fn hydrate(&self, path: &Utf8Path) -> Result<(), VfsError> {
        let cmd = XpcCommand::Hydrate {
            path: self.rel_path(path),
        };
        let reply = self.send_cmd(cmd).map_err(VfsError::from)?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }

    async fn dehydrate(&self, path: &Utf8Path) -> Result<(), VfsError> {
        let cmd = XpcCommand::Dehydrate {
            path: self.rel_path(path),
        };
        let reply = self.send_cmd(cmd).map_err(VfsError::from)?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }

    async fn is_virtual(&self, path: &Utf8Path) -> Result<bool, VfsError> {
        let cmd = XpcCommand::IsVirtual {
            path: self.rel_path(path),
        };
        let reply = self.send_cmd(cmd).map_err(VfsError::from)?;
        if !reply.ok {
            return Err(VfsError::from(VfsMacOsError::Protocol(
                reply.error.unwrap_or_else(|| "unknown error".to_string()),
            )));
        }
        Ok(reply.bool_value.unwrap_or(false))
    }

    async fn status(&self, path: &Utf8Path) -> Result<VfsStatus, VfsError> {
        let cmd = XpcCommand::Status {
            path: self.rel_path(path),
        };
        let reply = self.send_cmd(cmd).map_err(VfsError::from)?;
        if !reply.ok {
            return Err(VfsError::from(VfsMacOsError::Protocol(
                reply.error.unwrap_or_else(|| "unknown error".to_string()),
            )));
        }
        let status_str = reply.status.ok_or_else(|| {
            VfsError::from(VfsMacOsError::Protocol("missing status field".to_string()))
        })?;
        match status_str.as_str() {
            "Hydrated" => Ok(VfsStatus::Full),
            "Dehydrated" | "Virtual" => Ok(VfsStatus::Placeholder),
            "Syncing" => Ok(VfsStatus::Syncing),
            other => Err(VfsError::from(VfsMacOsError::Protocol(format!(
                "unrecognized VfsStatus: '{other}'"
            )))),
        }
    }

    async fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<(), VfsError> {
        let cmd = XpcCommand::SetPinned {
            path: self.rel_path(path),
            pinned,
        };
        let reply = self.send_cmd(cmd).map_err(VfsError::from)?;
        Self::expect_ok(reply).map_err(VfsError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VfsMacOs>();
    }
}

//! `VfsWindows` — the public struct implementing `vfs_core::Vfs` on Windows.

use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use tokio::sync::mpsc;

use vfs_core::{Vfs, VfsError, VfsFileItem, VfsStatus};

use crate::callback::{
    register_hydration_callback, unregister_hydration_callback, HydrationCallbackContext,
    HydrationRequest,
};
use crate::hydration;
use crate::pin;
use crate::placeholder;
use crate::registration::{register_sync_root, unregister_sync_root};

use windows::Win32::Storage::CloudFilters::CF_CONNECTION_KEY;

/// Provider name registered with the CfAPI sync root.
const PROVIDER_NAME: &str = "ownCloud";

/// Capacity of the internal hydration-request channel.
const HYDRATION_CHANNEL_CAPACITY: usize = 64;

/// A VFS implementation backed by the Windows CloudFiles API.
///
/// On construction (`new`) this registers a CfAPI sync root and installs a
/// `CF_CALLBACK_TYPE_FETCH_DATA` callback. On drop it removes the callback
/// and unregisters the sync root.
pub struct VfsWindows {
    root: Utf8PathBuf,
    callback_key: CF_CONNECTION_KEY,
    /// Receiver for hydration requests produced by the CfAPI callback. Held so
    /// the channel stays open for the lifetime of the sync root; servicing it
    /// (forwarding requests to the sync engine) is not yet wired up.
    #[allow(dead_code)]
    hydration_rx: mpsc::Receiver<HydrationRequest>,
}

impl VfsWindows {
    /// Create a new `VfsWindows` for `root`.
    ///
    /// Calls `CfRegisterSyncRoot` then `CfConnectSyncRoot`. Hydration requests
    /// from Windows are forwarded onto an internal channel whose receiver is
    /// retained by the returned `VfsWindows`.
    ///
    /// # Errors
    ///
    /// Returns [`VfsError`] if either CfAPI call fails.
    pub fn new(root: Utf8PathBuf) -> Result<Self, VfsError> {
        register_sync_root(&root, PROVIDER_NAME, "1.0.0").map_err(VfsError::from)?;

        let (hydration_tx, hydration_rx) = mpsc::channel(HYDRATION_CHANNEL_CAPACITY);

        let ctx = Arc::new(HydrationCallbackContext { tx: hydration_tx });

        let callback_key = register_hydration_callback(&root, ctx).map_err(VfsError::from)?;

        Ok(Self {
            root,
            callback_key,
            hydration_rx,
        })
    }
}

impl Drop for VfsWindows {
    fn drop(&mut self) {
        if let Err(e) = unregister_hydration_callback(self.callback_key) {
            eprintln!("vfs-windows: failed to unregister hydration callback: {e}");
        }
        if let Err(e) = unregister_sync_root(&self.root) {
            eprintln!("vfs-windows: failed to unregister sync root: {e}");
        }
    }
}

#[async_trait::async_trait]
impl Vfs for VfsWindows {
    async fn create_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError> {
        let root = self.root.clone();
        let item = item.clone();
        tokio::task::spawn_blocking(move || {
            placeholder::create_placeholder(&root, &item).map_err(VfsError::from)
        })
        .await
        .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    async fn update_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError> {
        let full_path = self.root.join(&item.path);
        let item = item.clone();
        tokio::task::spawn_blocking(move || {
            placeholder::update_placeholder(&full_path, &item).map_err(VfsError::from)
        })
        .await
        .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    async fn hydrate(&self, path: &Utf8Path) -> Result<(), VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || hydration::hydrate(&path).map_err(VfsError::from))
            .await
            .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    async fn dehydrate(&self, path: &Utf8Path) -> Result<(), VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || hydration::dehydrate(&path).map_err(VfsError::from))
            .await
            .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    async fn is_virtual(&self, path: &Utf8Path) -> Result<bool, VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || hydration::is_virtual(&path).map_err(VfsError::from))
            .await
            .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    async fn status(&self, path: &Utf8Path) -> Result<VfsStatus, VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || hydration::status(&path).map_err(VfsError::from))
            .await
            .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    async fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<(), VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || pin::set_pinned(&path, pinned).map_err(VfsError::from))
            .await
            .map_err(|e| VfsError::Backend(e.to_string()))?
    }
}

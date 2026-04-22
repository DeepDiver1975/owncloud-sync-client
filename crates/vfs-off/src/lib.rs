use async_trait::async_trait;
use camino::Utf8Path;
use vfs_core::{Vfs, VfsError, VfsFileItem, VfsStatus};

/// A VFS implementation that performs no operations.
///
/// `status()` always returns [`VfsStatus::Full`], modelling an environment
/// where all files are already present on disk and no dehydration is possible.
#[derive(Debug, Default, Clone)]
pub struct VfsOff;

impl VfsOff {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Vfs for VfsOff {
    async fn create_placeholder(
        &self,
        _path: &Utf8Path,
        _item: &VfsFileItem,
    ) -> Result<(), VfsError> {
        Ok(())
    }

    async fn hydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> {
        Ok(())
    }

    async fn dehydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> {
        Ok(())
    }

    async fn status(&self, _path: &Utf8Path) -> Result<VfsStatus, VfsError> {
        Ok(VfsStatus::Full)
    }

    async fn set_pinned(&self, _path: &Utf8Path, _pinned: bool) -> Result<(), VfsError> {
        Ok(())
    }
}

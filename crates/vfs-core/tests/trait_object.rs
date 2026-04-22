use vfs_core::{Vfs, VfsError, VfsFileItem, VfsStatus};
use camino::Utf8Path;

struct Dummy;

#[async_trait::async_trait]
impl Vfs for Dummy {
    async fn create_placeholder(&self, _item: &VfsFileItem) -> Result<(), VfsError> {
        Ok(())
    }
    async fn update_placeholder(&self, _item: &VfsFileItem) -> Result<(), VfsError> {
        Ok(())
    }
    async fn hydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> {
        Ok(())
    }
    async fn dehydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> {
        Ok(())
    }
    async fn is_virtual(&self, _path: &Utf8Path) -> Result<bool, VfsError> {
        Ok(false)
    }
    async fn status(&self, _path: &Utf8Path) -> Result<VfsStatus, VfsError> {
        Ok(VfsStatus::Full)
    }
    async fn set_pinned(&self, _path: &Utf8Path, _pinned: bool) -> Result<(), VfsError> {
        Ok(())
    }
}

#[tokio::test]
async fn trait_object_compiles() {
    let d: Box<dyn Vfs + Send + Sync> = Box::new(Dummy);
    let path = Utf8Path::new("/tmp/test.txt");
    let item = VfsFileItem {
        path: path.to_owned(),
        size: 0,
        etag: String::new(),
        file_id: String::new(),
        last_modified: std::time::SystemTime::UNIX_EPOCH,
    };
    d.create_placeholder(&item).await.unwrap();
    let s = d.status(path).await.unwrap();
    assert_eq!(s, VfsStatus::Full);
}

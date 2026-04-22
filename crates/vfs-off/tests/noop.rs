use camino::Utf8Path;
use vfs_core::{Vfs, VfsFileItem, VfsStatus};
use vfs_off::VfsOff;

fn item(path: &Utf8Path) -> VfsFileItem {
    VfsFileItem {
        path: path.to_owned(),
        size: 42,
        etag: "abc".into(),
        file_id: "id1".into(),
    }
}

#[tokio::test]
async fn all_methods_return_ok() {
    let vfs = VfsOff::new();
    let p = Utf8Path::new("/tmp/foo.txt");

    vfs.create_placeholder(p, &item(p)).await.unwrap();
    vfs.hydrate(p).await.unwrap();
    vfs.dehydrate(p).await.unwrap();
    vfs.set_pinned(p, true).await.unwrap();

    let s = vfs.status(p).await.unwrap();
    assert_eq!(s, VfsStatus::Full);
}

#[tokio::test]
async fn vfs_off_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<VfsOff>();
}

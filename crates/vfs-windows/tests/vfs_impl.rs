#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use std::time::SystemTime;
    use vfs_core::{Vfs, VfsFileItem, VfsStatus};
    use vfs_windows::VfsWindows;

    fn make_item(name: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size: 512,
            etag: "etag-vfs".into(),
            file_id: "fid-vfs".into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// Full lifecycle: VfsWindows::new → create_placeholder → is_virtual →
    /// set_pinned → dehydrate → drop (auto-unregisters).
    #[tokio::test]
    #[ignore = "requires Windows + NTFS volume"]
    async fn full_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap().to_owned();

        let vfs = VfsWindows::new(root.clone()).expect("VfsWindows::new should succeed");

        let item = make_item("test.txt");
        vfs.create_placeholder(&item)
            .await
            .expect("create_placeholder");

        let file = root.join("test.txt");
        let virtual_flag = vfs.is_virtual(&file).await.unwrap();
        assert!(virtual_flag, "newly created placeholder must be virtual");

        let s = vfs.status(&file).await.unwrap();
        assert_eq!(s, VfsStatus::Placeholder);

        vfs.set_pinned(&file, true).await.expect("set_pinned(true)");
        vfs.set_pinned(&file, false)
            .await
            .expect("set_pinned(false)");
        vfs.dehydrate(&file).await.expect("dehydrate");

        drop(vfs);
    }

    /// VfsWindows must be usable as a trait object.
    #[test]
    fn vfs_windows_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VfsWindows>();
    }
}

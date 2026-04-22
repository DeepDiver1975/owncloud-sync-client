#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use std::time::SystemTime;
    use vfs_core::VfsFileItem;
    use vfs_windows::pin::set_pinned;
    use vfs_windows::placeholder::create_placeholder;
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    fn make_item(name: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size: 256,
            etag: "etag-pin".into(),
            file_id: "fid-pin".into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// Pin a placeholder, then unpin it — both operations must succeed.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn pin_then_unpin_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();
        create_placeholder(root, &make_item("pinme.txt")).unwrap();

        let file = root.join("pinme.txt");

        set_pinned(&file, true).expect("set_pinned(true) should succeed");
        set_pinned(&file, false).expect("set_pinned(false) should succeed");

        unregister_sync_root(root).unwrap();
    }

    /// Pinning a non-existent file must return an error, not panic.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn pin_nonexistent_returns_error() {
        let file = Utf8Path::new("C:\\nonexistent_vfs_test_file_xyz.txt");
        let result = set_pinned(file, true);
        assert!(result.is_err(), "pinning non-existent file should fail");
    }
}

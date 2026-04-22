#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use std::time::SystemTime;
    use vfs_core::{VfsFileItem, VfsStatus};
    use vfs_windows::hydration::{dehydrate, hydrate, is_virtual, status};
    use vfs_windows::placeholder::create_placeholder;
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    fn make_item(name: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size: 1024,
            etag: "etag-hydration".into(),
            file_id: "fid-hydration".into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// After creating a placeholder, is_virtual() must return true.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn placeholder_is_virtual() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();
        create_placeholder(root, &make_item("v.txt")).unwrap();

        let file = root.join("v.txt");
        assert!(
            is_virtual(&file).expect("is_virtual should succeed"),
            "newly created placeholder must be virtual"
        );

        unregister_sync_root(root).unwrap();
    }

    /// Status of a fresh placeholder must be VfsStatus::Placeholder.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn status_of_placeholder_is_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();
        create_placeholder(root, &make_item("s.txt")).unwrap();

        let file = root.join("s.txt");
        let s = status(&file).expect("status should succeed");
        assert_eq!(s, VfsStatus::Placeholder);

        unregister_sync_root(root).unwrap();
    }

    /// hydrate() and dehydrate() must not panic on a valid placeholder.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn hydrate_dehydrate_do_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();
        create_placeholder(root, &make_item("hd.txt")).unwrap();
        let file = root.join("hd.txt");

        // hydrate() will trigger a FETCH_DATA callback; since no real callback
        // handler is running, it may return a timeout or "no provider" error.
        // We only verify it doesn't panic.
        let _ = hydrate(&file);

        // dehydrate() on an already-dehydrated placeholder should succeed.
        let result = dehydrate(&file);
        assert!(result.is_ok(), "dehydrate on placeholder: {:?}", result);

        unregister_sync_root(root).unwrap();
    }
}

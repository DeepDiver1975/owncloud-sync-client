#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use std::time::SystemTime;
    use vfs_core::VfsFileItem;
    use vfs_windows::placeholder::{create_placeholder, update_placeholder};
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    fn make_item(name: &str, size: u64, file_id: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size,
            etag: "etag-test".into(),
            file_id: file_id.into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// Creates a sync root, creates a placeholder file, then verifies the file
    /// appears on disk with the correct size (0 bytes — it is a placeholder).
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn create_placeholder_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();

        let item = make_item("hello.txt", 1024, "file-id-001");
        create_placeholder(root, &item).expect("create_placeholder should succeed");

        let file_path = dir.path().join("hello.txt");
        assert!(file_path.exists(), "placeholder file should exist on disk");
        // Placeholder is 0 bytes on disk until hydrated.
        assert_eq!(file_path.metadata().unwrap().len(), 0);

        unregister_sync_root(root).unwrap();
    }

    /// Creates a placeholder then updates its metadata (new size + etag).
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn update_placeholder_changes_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();

        let item = make_item("update_me.txt", 512, "file-id-002");
        create_placeholder(root, &item).unwrap();

        let updated = make_item("update_me.txt", 2048, "file-id-002");
        let full_path = root.join("update_me.txt");
        update_placeholder(&full_path, &updated).expect("update_placeholder should succeed");

        unregister_sync_root(root).unwrap();
    }
}

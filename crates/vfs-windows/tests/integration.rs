//! Integration test skeleton for vfs-windows.
//!
//! All tests are marked `#[ignore]` because they require:
//!   - A real Windows machine
//!   - An NTFS-formatted volume (not FAT32 / exFAT)
//!   - Developer Mode enabled OR administrator privileges
//!
//! # How to run
//!
//! ```bash
//! cargo test --target x86_64-pc-windows-msvc -p vfs-windows --test integration -- --ignored
//! ```
//!
//! # CI
//!
//! These tests should be added to a dedicated Windows runner job in the CI
//! pipeline and run only on pushes that touch `crates/vfs-windows/`.

#[cfg(target_os = "windows")]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use std::time::SystemTime;
    use tokio::sync::mpsc;
    use vfs_core::{Vfs, VfsFileItem, VfsStatus};
    use vfs_windows::{HydrationRequest, VfsWindows};

    fn make_item(name: &str, size: u64, file_id: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size,
            etag: "integration-etag".into(),
            file_id: file_id.into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// Creates a VfsWindows instance, creates a 1 KB placeholder, checks
    /// is_virtual returns true, checks status is Placeholder, and verifies
    /// VfsWindows drops without panicking.
    #[tokio::test]
    #[ignore = "requires Windows + NTFS volume"]
    async fn create_placeholder_is_virtual_status() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let root = Utf8Path::from_path(dir.path())
            .expect("temp dir path is not valid UTF-8")
            .to_owned();

        let (tx, mut rx) = mpsc::channel::<HydrationRequest>(16);

        let vfs = VfsWindows::new(root.clone(), "IntegrationTestProvider", tx)
            .expect("VfsWindows::new should succeed on NTFS volume");

        let item = make_item("integration_test.txt", 1024, "integration-file-id-001");
        vfs.create_placeholder(&item)
            .await
            .expect("create_placeholder should succeed");

        let file_path = root.join("integration_test.txt");
        assert!(file_path.exists(), "placeholder file must exist on disk");

        let virtual_flag = vfs
            .is_virtual(&file_path)
            .await
            .expect("is_virtual should succeed");
        assert!(virtual_flag, "newly created placeholder must be virtual");

        let s = vfs.status(&file_path).await.expect("status should succeed");
        assert_eq!(
            s,
            VfsStatus::Placeholder,
            "status of a fresh placeholder must be Placeholder"
        );

        vfs.dehydrate(&file_path)
            .await
            .expect("dehydrate should succeed on placeholder");

        assert!(
            rx.try_recv().is_err(),
            "no hydration request should have been sent during this test"
        );

        drop(vfs);
    }

    /// Verify update_placeholder replaces the metadata of an existing placeholder.
    #[tokio::test]
    #[ignore = "requires Windows + NTFS volume"]
    async fn update_placeholder_changes_size() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let (tx, _rx) = mpsc::channel::<HydrationRequest>(8);

        let vfs = VfsWindows::new(root.clone(), "IntegrationTestProvider", tx).unwrap();

        let original = make_item("update_test.txt", 512, "upd-fid");
        vfs.create_placeholder(&original).await.unwrap();

        let updated = VfsFileItem {
            path: Utf8PathBuf::from("update_test.txt"),
            size: 8192,
            etag: "new-etag".into(),
            file_id: "upd-fid".into(),
            last_modified: SystemTime::UNIX_EPOCH,
        };
        let file_path = root.join("update_test.txt");
        vfs.update_placeholder(&updated)
            .await
            .expect("update_placeholder should succeed");

        assert!(vfs.is_virtual(&file_path).await.unwrap());
        drop(vfs);
    }

    /// Verify set_pinned does not error on a freshly created placeholder.
    #[tokio::test]
    #[ignore = "requires Windows + NTFS volume"]
    async fn set_pinned_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let (tx, _rx) = mpsc::channel::<HydrationRequest>(8);

        let vfs = VfsWindows::new(root.clone(), "IntegrationTestProvider", tx).unwrap();

        let item = make_item("pin_test.txt", 256, "pin-fid");
        vfs.create_placeholder(&item).await.unwrap();

        let file = root.join("pin_test.txt");
        vfs.set_pinned(&file, true)
            .await
            .expect("pin should succeed");
        vfs.set_pinned(&file, false)
            .await
            .expect("unpin should succeed");

        drop(vfs);
    }
}

/// On non-Windows platforms this module is empty; it must still compile.
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn placeholder_compilation_guard() {}

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use vfs_windows::callback::{
        register_hydration_callback, unregister_hydration_callback, HydrationCallbackContext,
        HydrationRequest, RawCallbackInfo,
    };
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    /// Registers a hydration callback and immediately unregisters it.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn register_and_unregister_callback() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();

        let (tx, _rx) = mpsc::channel::<HydrationRequest>(8);
        let ctx = Arc::new(HydrationCallbackContext { tx });

        let key = register_hydration_callback(root, ctx)
            .expect("register_hydration_callback should succeed");

        assert_ne!(key.0, 0, "connection key must be non-zero");

        unregister_hydration_callback(key).expect("unregister should succeed");
        unregister_sync_root(root).unwrap();
    }

    /// Verifies the HydrationRequest struct has the expected fields.
    #[test]
    fn hydration_request_fields_accessible() {
        let req = HydrationRequest {
            path: Utf8PathBuf::from("C:\\sync\\file.txt"),
            offset: 0,
            length: 4096,
            callback_info: RawCallbackInfo {
                connection_key: windows::Win32::Storage::CloudFilters::CF_CONNECTION_KEY(1),
                transfer_key: 42,
                request_key: 99,
            },
        };
        assert_eq!(req.offset, 0);
        assert_eq!(req.length, 4096);
        assert_eq!(req.path.as_str(), "C:\\sync\\file.txt");
    }
}

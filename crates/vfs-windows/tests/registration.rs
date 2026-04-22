#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
mod tests {
    use camino::Utf8Path;
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    #[test]
    #[ignore = "requires Windows with CfAPI and a real path"]
    fn register_and_unregister_round_trip() {
        let path = Utf8Path::new("C:\\SyncRoot\\Test");
        register_sync_root(path, "TestProvider", "1.0").unwrap();
        unregister_sync_root(path).unwrap();
    }
}

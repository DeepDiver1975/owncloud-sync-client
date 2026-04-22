//! Integration tests for vfs-macos.
//!
//! All tests in this file require a macOS system with the FileProvider
//! extension running and are therefore marked `#[ignore]`.

#[cfg(test)]
mod integration {
    use camino::Utf8PathBuf;

    #[cfg(target_os = "macos")]
    use vfs_macos::VfsMacOs;
    #[cfg(target_os = "macos")]
    use vfs_core::Vfs;

    /// Verify that `VfsMacOs::new` can connect to the XPC service.
    ///
    /// Pre-conditions:
    ///   - macOS 12+
    ///   - ownCloud FileProvider extension registered and running
    ///   - Sync root exists at ~/ownCloud
    #[test]
    #[ignore = "requires macOS + running FileProvider extension"]
    #[cfg(target_os = "macos")]
    fn test_is_virtual_smoke() {
        let home = std::env::var("HOME").expect("HOME not set");
        let root = Utf8PathBuf::from(format!("{home}/ownCloud"));
        let vfs  = VfsMacOs::new(root.clone()).expect("VfsMacOs::new failed");

        let test_path = root.join("integration_test_placeholder.txt");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(vfs.is_virtual(&test_path));

        assert!(
            result.is_ok(),
            "is_virtual returned an error: {:?}",
            result
        );
        println!("is_virtual({test_path}) = {:?}", result.unwrap());
    }

    /// Verify that `hydrate` and `dehydrate` round-trip without error.
    #[test]
    #[ignore = "requires macOS + running FileProvider extension"]
    #[cfg(target_os = "macos")]
    fn test_hydrate_dehydrate_roundtrip() {
        let home = std::env::var("HOME").expect("HOME not set");
        let root = Utf8PathBuf::from(format!("{home}/ownCloud"));
        let vfs  = VfsMacOs::new(root.clone()).expect("VfsMacOs::new failed");
        let path = root.join("integration_test_placeholder.txt");

        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(vfs.dehydrate(&path)).expect("dehydrate failed");
        assert!(
            rt.block_on(vfs.is_virtual(&path)).expect("is_virtual after dehydrate"),
            "should be virtual after dehydrate"
        );

        rt.block_on(vfs.hydrate(&path)).expect("hydrate failed");
        assert!(
            !rt.block_on(vfs.is_virtual(&path)).expect("is_virtual after hydrate"),
            "should not be virtual after hydrate"
        );
    }
}

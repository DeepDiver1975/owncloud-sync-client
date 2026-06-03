#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
mod tests {
    use camino::Utf8PathBuf;
    use vfs_core::VfsError;
    use vfs_windows::error::VfsWindowsError;

    #[test]
    fn path_not_found_converts_to_vfs_not_found() {
        let path = Utf8PathBuf::from("/some/path");
        let err = VfsWindowsError::PathNotFound(path.clone());
        let vfs_err: VfsError = err.into();
        assert!(matches!(vfs_err, VfsError::NotFound { path: p } if p == path));
    }

    #[test]
    fn not_supported_converts_to_vfs_not_supported() {
        let err = VfsWindowsError::NotSupported("test".to_string());
        let vfs_err: VfsError = err.into();
        assert!(matches!(vfs_err, VfsError::NotSupported));
    }

    #[test]
    fn io_error_converts_to_vfs_io() {
        let io_err = std::io::Error::other("test");
        let err = VfsWindowsError::Io(io_err);
        let vfs_err: VfsError = err.into();
        assert!(matches!(vfs_err, VfsError::Io(_)));
    }

    #[test]
    fn backend_converts_to_vfs_backend() {
        let err = VfsWindowsError::Backend("some backend error".to_string());
        let vfs_err: VfsError = err.into();
        assert!(matches!(vfs_err, VfsError::Backend(_)));
    }
}

use camino::Utf8Path;
use std::sync::Arc;
use thiserror::Error;
use vfs_core::Vfs;
use vfs_off::VfsOff;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("VFS mode '{0}' is not supported on this platform")]
    VfsNotSupported(String),
    #[error("unknown VFS mode: '{0}'")]
    UnknownVfsMode(String),
    #[error("VFS initialisation error: {0}")]
    VfsInit(String),
}

pub fn create_vfs(mode: &str, root: &Utf8Path) -> Result<Arc<dyn Vfs>, DaemonError> {
    match mode {
        "off" => Ok(Arc::new(VfsOff::new())),

        "windows_cf" => {
            #[cfg(target_os = "windows")]
            {
                use vfs_windows::VfsWindows;
                let vfs = VfsWindows::new(root).map_err(|e| DaemonError::VfsInit(e.to_string()))?;
                Ok(Arc::new(vfs))
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = root;
                Err(DaemonError::VfsNotSupported(
                    "windows_cf requires Windows".into(),
                ))
            }
        }

        "macos_fp" => {
            #[cfg(target_os = "macos")]
            {
                use vfs_macos::VfsMacOs;
                let vfs =
                    VfsMacOs::new(root.into()).map_err(|e| DaemonError::VfsInit(e.to_string()))?;
                Ok(Arc::new(vfs))
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = root;
                Err(DaemonError::VfsNotSupported(
                    "macos_fp requires macOS".into(),
                ))
            }
        }

        other => Err(DaemonError::UnknownVfsMode(other.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    fn temp_utf8_path() -> Utf8PathBuf {
        let dir = tempdir().unwrap();
        let path = dir.keep();
        Utf8PathBuf::from(path.to_string_lossy().into_owned())
    }

    #[test]
    fn off_mode_works_on_all_platforms() {
        let path = temp_utf8_path();
        assert!(create_vfs("off", &path).is_ok());
    }

    #[test]
    fn unknown_mode_returns_error() {
        let path = temp_utf8_path();
        let result = create_vfs("fuse_magic", &path);
        assert!(matches!(result, Err(DaemonError::UnknownVfsMode(_))));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn windows_cf_unsupported_on_non_windows() {
        let path = temp_utf8_path();
        let result = create_vfs("windows_cf", &path);
        assert!(matches!(result, Err(DaemonError::VfsNotSupported(_))));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn macos_fp_unsupported_on_non_macos() {
        let path = temp_utf8_path();
        let result = create_vfs("macos_fp", &path);
        assert!(matches!(result, Err(DaemonError::VfsNotSupported(_))));
    }
}

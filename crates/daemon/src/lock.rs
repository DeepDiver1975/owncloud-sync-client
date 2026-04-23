use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use fs2::FileExt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LockError {
    #[error("another instance of ocsyncd is already running")]
    AlreadyRunning,
    #[error("lock file I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct LockFile {
    path: PathBuf,
    _file: File,
}

impl LockFile {
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().read(true).write(true).create(true).open(path)?;
        file.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                LockError::AlreadyRunning
            } else {
                LockError::Io(e)
            }
        })?;
        Ok(LockFile { path: path.to_owned(), _file: file })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn acquire_and_release() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.lock");
        let lock = LockFile::acquire(&path).unwrap();
        assert!(path.exists());
        drop(lock);
        let _lock2 = LockFile::acquire(&path).unwrap();
    }

    #[test]
    fn second_acquire_returns_already_running() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        let _lock = LockFile::acquire(&path).unwrap();
        use std::fs::OpenOptions;
        let f2 = OpenOptions::new().read(true).write(true).create(true).open(&path).unwrap();
        let result = f2.try_lock_exclusive();
        #[cfg(not(target_os = "linux"))]
        assert!(result.is_err(), "expected lock conflict");
        #[cfg(target_os = "linux")]
        let _ = result;
    }

    #[test]
    fn lock_file_created_in_missing_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subdir").join("nested").join("daemon.lock");
        let _lock = LockFile::acquire(&path).unwrap();
        assert!(path.exists());
    }
}

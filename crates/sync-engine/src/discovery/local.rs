use camino::{Utf8Path, Utf8PathBuf};
use std::os::unix::fs::MetadataExt as _;
use std::time::SystemTime;

use crate::error::Result;
use crate::types::LocalEntry;

/// Walk `root` recursively and return one [`LocalEntry`] per **file** found.
///
/// Directory entries are skipped; only regular files are returned.
/// The walk runs on a blocking thread pool via `spawn_blocking`.
pub async fn discover_local(root: &Utf8Path) -> Result<Vec<LocalEntry>> {
    let root = root.to_owned();
    tokio::task::spawn_blocking(move || walk(&root))
        .await
        .map_err(|e| crate::error::SyncError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        )))?
}

fn walk(root: &Utf8Path) -> Result<Vec<LocalEntry>> {
    let mut entries = Vec::new();
    walk_dir(root, &mut entries)?;
    Ok(entries)
}

fn walk_dir(dir: &Utf8Path, entries: &mut Vec<LocalEntry>) -> Result<()> {
    let read_dir = std::fs::read_dir(dir)?;

    let mut subdirs = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        let meta = entry.metadata()?;
        let path = Utf8PathBuf::from_path_buf(entry.path())
            .unwrap_or_else(|p| Utf8PathBuf::from(p.to_string_lossy().as_ref()));

        if meta.is_dir() {
            subdirs.push(path);
        } else if meta.is_file() {
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let inode = meta.ino();
            entries.push(LocalEntry {
                path,
                mtime,
                size: meta.len(),
                inode,
                is_virtual: false,
            });
        }
    }

    for sub in subdirs {
        walk_dir(&sub, entries)?;
    }

    Ok(())
}

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::SystemTime;

fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

use camino::Utf8PathBuf;
use tokio::task::JoinSet;
use url::Url;
use uuid::Uuid;

use crate::discovery::local::discover_local;
use crate::discovery::remote::discover_remote;
use crate::error::{Result, SyncError};
use crate::propagate::download::{propagate_download, DownloadRequest};
use crate::propagate::upload::{propagate_upload, UploadRequest};
use crate::reconcile::{reconcile, JournalBaseline};
use crate::state::{FileStatus, FolderStatus, SyncState};
use crate::types::{ConflictStrategy, LocalEntry, RemoteEntry, SyncInstruction};
use sync_db::{JournalEntry, SyncJournalDb};

pub struct EngineConfig {
    pub folder_id: Uuid,
    pub local_root: Utf8PathBuf,
    pub space_root: Url,
    pub conflict_strategy: ConflictStrategy,
    pub max_parallel_transfers: usize,
    pub db: SyncJournalDb,
}

pub struct SyncEngine {
    cfg: EngineConfig,
    state: Arc<RwLock<SyncState>>,
}

impl SyncEngine {
    pub fn new(cfg: EngineConfig) -> Self {
        let state = Arc::new(RwLock::new(SyncState::new(cfg.folder_id)));
        Self { cfg, state }
    }

    pub async fn run_sync(&self) -> Result<()> {
        {
            let mut s = write_lock(&self.state);
            s.status = FolderStatus::Syncing;
        }

        // Phase 1: Discovery
        tracing::info!("discover_remote: {}", self.cfg.space_root);
        let (local_entries, remote_entries) = tokio::try_join!(
            discover_local(&self.cfg.local_root),
            discover_remote(&self.cfg.space_root),
        )?;

        let local_map: HashMap<Utf8PathBuf, LocalEntry> = local_entries
            .into_iter()
            .map(|e| {
                let rel = e
                    .path
                    .strip_prefix(&self.cfg.local_root)
                    .unwrap_or(&e.path)
                    .to_owned();
                (rel, e)
            })
            .collect();

        let remote_map: HashMap<Utf8PathBuf, RemoteEntry> = remote_entries
            .into_iter()
            .map(|e| (e.path.clone(), e))
            .collect();

        let mut all_paths: std::collections::HashSet<Utf8PathBuf> =
            local_map.keys().cloned().collect();
        all_paths.extend(remote_map.keys().cloned());

        // Phase 2: Reconcile — load journal baselines from DB.
        let mut instructions: Vec<(Utf8PathBuf, SyncInstruction, Option<RemoteEntry>)> = Vec::new();
        for path in all_paths {
            let loc = local_map.get(&path).cloned();
            let rem = remote_map.get(&path).cloned();
            let journal = self
                .cfg
                .db
                .get_entry(path.as_str())
                .await
                .ok()
                .flatten()
                .and_then(|e| {
                    let etag = e.etag?;
                    let size = e.size? as u64;
                    Some((etag, size) as JournalBaseline)
                });
            let instr = reconcile(loc, rem.clone(), journal, self.cfg.conflict_strategy);
            if instr != SyncInstruction::Ignore {
                instructions.push((path, instr, rem));
            }
        }

        // Phase 3: Propagate
        let mut join_set: JoinSet<Result<()>> = JoinSet::new();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.cfg.max_parallel_transfers));

        for (rel_path, instruction, rem_entry) in instructions {
            let local_path = self.cfg.local_root.join(&rel_path);
            let remote_url = self
                .cfg
                .space_root
                .join(rel_path.as_str())
                .map_err(|e| SyncError::Parse(e.to_string()))?;

            let sem = semaphore.clone();
            let state = self.state.clone();
            let rel_clone = rel_path.clone();
            let db = self.cfg.db.clone();

            match instruction {
                SyncInstruction::Download => {
                    join_set.spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        {
                            let mut s = write_lock(&state);
                            s.set_file_status(rel_clone.clone(), FileStatus::Syncing);
                        }
                        let req = DownloadRequest {
                            remote_url,
                            local_dest: local_path.clone(),
                            expected_etag: None,
                        };
                        match propagate_download(req).await {
                            Ok(etag) => {
                                // Record journal baseline after successful download.
                                let size = tokio::fs::metadata(&local_path)
                                    .await
                                    .map(|m| m.len())
                                    .unwrap_or(0);
                                let entry = JournalEntry {
                                    path: rel_clone.to_string(),
                                    etag: Some(etag.trim_matches('"').to_string()),
                                    mtime: None,
                                    size: Some(size as i64),
                                    inode: None,
                                    file_id: rem_entry.as_ref().map(|r| r.file_id.clone()),
                                    checksum: None,
                                    is_virtual: 0,
                                };
                                let _ = db.upsert_entry(&entry).await;
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Ok);
                                Ok(())
                            }
                            Err(e) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Error(e.to_string()));
                                Err(e)
                            }
                        }
                    });
                }

                SyncInstruction::Upload => {
                    join_set.spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        {
                            let mut s = write_lock(&state);
                            s.set_file_status(rel_clone.clone(), FileStatus::Syncing);
                        }
                        let size = tokio::fs::metadata(&local_path)
                            .await
                            .map(|m| m.len())
                            .unwrap_or(0);
                        let req = UploadRequest {
                            local_path: local_path.clone(),
                            remote_url,
                            size,
                            checksum: None,
                            tus_threshold: 5 * 1024 * 1024,
                        };
                        match propagate_upload(req).await {
                            Ok(etag) => {
                                // Record journal baseline after successful upload.
                                let entry = JournalEntry {
                                    path: rel_clone.to_string(),
                                    etag: Some(etag.trim_matches('"').to_string()),
                                    mtime: None,
                                    size: Some(size as i64),
                                    inode: None,
                                    file_id: None,
                                    checksum: None,
                                    is_virtual: 0,
                                };
                                let _ = db.upsert_entry(&entry).await;
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Ok);
                                Ok(())
                            }
                            Err(e) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Error(e.to_string()));
                                Err(e)
                            }
                        }
                    });
                }

                SyncInstruction::Conflict => {
                    // Keep both: rename local file to a conflict copy, then download remote.
                    join_set.spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        {
                            let mut s = write_lock(&state);
                            s.set_file_status(rel_clone.clone(), FileStatus::Syncing);
                        }

                        // Derive conflict filename: insert timestamp before extension.
                        let conflict_path = make_conflict_path(&local_path);
                        if let Err(e) = tokio::fs::rename(&local_path, &conflict_path).await {
                            let mut s = write_lock(&state);
                            s.set_file_status(
                                rel_clone,
                                FileStatus::Error(format!("conflict rename: {e}")),
                            );
                            return Err(SyncError::Io(e));
                        }

                        // Download remote version to the original path.
                        let req = DownloadRequest {
                            remote_url,
                            local_dest: local_path.clone(),
                            expected_etag: None,
                        };
                        match propagate_download(req).await {
                            Ok(etag) => {
                                let size = tokio::fs::metadata(&local_path)
                                    .await
                                    .map(|m| m.len())
                                    .unwrap_or(0);
                                let entry = JournalEntry {
                                    path: rel_clone.to_string(),
                                    etag: Some(etag.trim_matches('"').to_string()),
                                    mtime: None,
                                    size: Some(size as i64),
                                    inode: None,
                                    file_id: rem_entry.as_ref().map(|r| r.file_id.clone()),
                                    checksum: None,
                                    is_virtual: 0,
                                };
                                let _ = db.upsert_entry(&entry).await;
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Ok);
                                Ok(())
                            }
                            Err(e) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Error(e.to_string()));
                                Err(e)
                            }
                        }
                    });
                }

                _ => {}
            }
        }

        let mut had_error = false;
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::warn!("Transfer error: {e}");
                    had_error = true;
                }
                Err(join_err) => {
                    tracing::error!("Task panicked: {join_err}");
                    had_error = true;
                }
            }
        }

        {
            let mut s = write_lock(&self.state);
            if had_error {
                s.status = FolderStatus::Error;
            } else {
                s.mark_complete();
            }
        }

        Ok(())
    }

    pub fn state(&self) -> Arc<RwLock<SyncState>> {
        self.state.clone()
    }
}

/// Build a conflict copy path by inserting a timestamp before the file extension.
/// e.g. `hello.txt` → `hello_conflict_20240501T120000.txt`
fn make_conflict_path(original: &Utf8PathBuf) -> Utf8PathBuf {
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let stem = original.file_stem().unwrap_or(original.as_str());
    let ext = original.extension();

    let conflict_name = match ext {
        Some(e) => format!("{stem}_conflict_{ts}.{e}"),
        None => format!("{stem}_conflict_{ts}"),
    };

    original
        .parent()
        .map(|p| p.join(&conflict_name))
        .unwrap_or_else(|| Utf8PathBuf::from(&conflict_name))
}

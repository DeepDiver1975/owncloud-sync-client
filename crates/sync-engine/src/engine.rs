use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

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

pub struct EngineConfig {
    pub folder_id: Uuid,
    pub local_root: Utf8PathBuf,
    pub space_root: Url,
    pub conflict_strategy: ConflictStrategy,
    pub max_parallel_transfers: usize,
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

        // Phase 2: Reconcile
        let instructions: Vec<(Utf8PathBuf, SyncInstruction)> = all_paths
            .into_iter()
            .map(|path| {
                let loc = local_map.get(&path).cloned();
                let rem = remote_map.get(&path).cloned();
                let journal: Option<JournalBaseline> = None;
                let instr = reconcile(loc, rem, journal, self.cfg.conflict_strategy);
                (path, instr)
            })
            .filter(|(_, instr)| *instr != SyncInstruction::Ignore)
            .collect();

        // Phase 3: Propagate
        let mut join_set: JoinSet<Result<()>> = JoinSet::new();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            self.cfg.max_parallel_transfers,
        ));

        for (rel_path, instruction) in instructions {
            let local_path = self.cfg.local_root.join(&rel_path);
            let remote_url = self
                .cfg
                .space_root
                .join(rel_path.as_str())
                .map_err(|e| SyncError::Parse(e.to_string()))?;

            let sem = semaphore.clone();
            let state = self.state.clone();
            let rel_clone = rel_path.clone();

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
                            local_dest: local_path,
                            expected_etag: None,
                        };
                        match propagate_download(req).await {
                            Ok(_etag) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Ok);
                                Ok(())
                            }
                            Err(e) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(
                                    rel_clone,
                                    FileStatus::Error(e.to_string()),
                                );
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
                            local_path,
                            remote_url,
                            size,
                            checksum: None,
                            tus_threshold: 5 * 1024 * 1024,
                        };
                        match propagate_upload(req).await {
                            Ok(_etag) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Ok);
                                Ok(())
                            }
                            Err(e) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(
                                    rel_clone,
                                    FileStatus::Error(e.to_string()),
                                );
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

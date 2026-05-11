use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::SystemTime;

fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

use camino::Utf8PathBuf;
use ocis_client::auth::TokenManager;
use tokio::task::JoinSet;
use url::Url;
use uuid::Uuid;

use crate::discovery::local::discover_local;
use crate::discovery::remote::discover_remote;
use crate::error::{Result, SyncError};
use crate::propagate::download::{propagate_download, DownloadRequest};
use crate::propagate::upload::{propagate_upload, UploadRequest};
use crate::reconcile::{reconcile, JournalBaseline};
use crate::report::{HttpEvent, SyncReport};
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
    pub token_manager: Arc<TokenManager>,
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

    pub async fn run_sync(&self) -> Result<SyncReport> {
        let t_start = tokio::time::Instant::now();
        let mut http_events: Vec<HttpEvent> = Vec::new();

        {
            let mut s = write_lock(&self.state);
            s.status = FolderStatus::Syncing;
        }

        // Phase 1: Discovery
        tracing::info!("discover_remote: {}", self.cfg.space_root);
        let bearer_token = self
            .cfg
            .token_manager
            .get_valid_token()
            .await
            .map_err(|e| SyncError::Auth(e.to_string()))?;
        let (local_entries, remote_entries) = tokio::try_join!(
            discover_local(&self.cfg.local_root),
            discover_remote(&self.cfg.space_root, &bearer_token, &mut http_events),
        )?;

        let remote_count = remote_entries.len();
        let local_count = local_entries.len();

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
        let mut instructions: Vec<(Utf8PathBuf, SyncInstruction, Option<RemoteEntry>)> = Vec::new();
        let mut n_ignored = 0usize;
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
            } else {
                n_ignored += 1;
            }
        }

        let n_downloads = instructions
            .iter()
            .filter(|(_, i, _)| *i == SyncInstruction::Download)
            .count();
        let n_uploads = instructions
            .iter()
            .filter(|(_, i, _)| *i == SyncInstruction::Upload)
            .count();
        let n_conflicts = instructions
            .iter()
            .filter(|(_, i, _)| *i == SyncInstruction::Conflict)
            .count();
        let n_del_local = instructions
            .iter()
            .filter(|(_, i, _)| *i == SyncInstruction::DeleteLocal)
            .count();
        let n_del_remote = instructions
            .iter()
            .filter(|(_, i, _)| *i == SyncInstruction::DeleteRemote)
            .count();

        // Phase 3: Propagate — each task owns its Vec<HttpEvent> and returns it.
        let mut join_set: JoinSet<(Vec<HttpEvent>, Result<()>)> = JoinSet::new();
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
                    let bearer_token_dl = bearer_token.clone();
                    join_set.spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        let mut task_http: Vec<HttpEvent> = Vec::new();
                        {
                            let mut s = write_lock(&state);
                            s.set_file_status(rel_clone.clone(), FileStatus::Syncing);
                        }
                        let req = DownloadRequest {
                            remote_url,
                            local_dest: local_path.clone(),
                            expected_etag: None,
                            bearer_token: bearer_token_dl,
                        };
                        let result = propagate_download(req, &mut task_http).await;
                        match result {
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
                                (task_http, Ok(()))
                            }
                            Err(e) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Error(e.to_string()));
                                (task_http, Err(e))
                            }
                        }
                    });
                }

                SyncInstruction::Upload => {
                    join_set.spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        let mut task_http: Vec<HttpEvent> = Vec::new();
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
                        let result = propagate_upload(req, &mut task_http).await;
                        match result {
                            Ok(etag) => {
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
                                (task_http, Ok(()))
                            }
                            Err(e) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Error(e.to_string()));
                                (task_http, Err(e))
                            }
                        }
                    });
                }

                SyncInstruction::Conflict => {
                    let bearer_token_cf = bearer_token.clone();
                    join_set.spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        let mut task_http: Vec<HttpEvent> = Vec::new();
                        {
                            let mut s = write_lock(&state);
                            s.set_file_status(rel_clone.clone(), FileStatus::Syncing);
                        }
                        let conflict_path = make_conflict_path(&local_path);
                        if let Err(e) = tokio::fs::rename(&local_path, &conflict_path).await {
                            let mut s = write_lock(&state);
                            s.set_file_status(
                                rel_clone,
                                FileStatus::Error(format!("conflict rename: {e}")),
                            );
                            return (task_http, Err(SyncError::Io(e)));
                        }
                        let req = DownloadRequest {
                            remote_url,
                            local_dest: local_path.clone(),
                            expected_etag: None,
                            bearer_token: bearer_token_cf,
                        };
                        let result = propagate_download(req, &mut task_http).await;
                        match result {
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
                                (task_http, Ok(()))
                            }
                            Err(e) => {
                                let mut s = write_lock(&state);
                                s.set_file_status(rel_clone, FileStatus::Error(e.to_string()));
                                (task_http, Err(e))
                            }
                        }
                    });
                }

                SyncInstruction::DeleteLocal => {
                    tracing::warn!("DeleteLocal not yet implemented for path {:?}", rel_path);
                }
                SyncInstruction::DeleteRemote => {
                    tracing::warn!("DeleteRemote not yet implemented for path {:?}", rel_path);
                }
                _ => {}
            }
        }

        let mut had_error = false;
        let mut error_messages: Vec<String> = Vec::new();
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok((task_http, Ok(()))) => {
                    http_events.extend(task_http);
                }
                Ok((task_http, Err(e))) => {
                    http_events.extend(task_http);
                    tracing::warn!("Transfer error: {e}");
                    error_messages.push(e.to_string());
                    had_error = true;
                }
                Err(join_err) => {
                    tracing::error!("Task panicked: {join_err}");
                    error_messages.push(format!("task panicked: {join_err}"));
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

        let duration_ms = t_start.elapsed().as_millis() as u64;

        let report = SyncReport {
            folder_id: self.cfg.folder_id,
            remote_entries: remote_count,
            local_entries: local_count,
            downloads: n_downloads,
            uploads: n_uploads,
            conflicts: n_conflicts,
            deletes_local: n_del_local,
            deletes_remote: n_del_remote,
            ignored: n_ignored,
            errors: error_messages,
            http_events,
            duration_ms,
        };

        tracing::info!(
            "sync done folder={} remote={} local={} dl={} ul={} conflict={} \
             del_local={} del_remote={} errors={} ms={}",
            self.cfg.folder_id,
            report.remote_entries,
            report.local_entries,
            report.downloads,
            report.uploads,
            report.conflicts,
            report.deletes_local,
            report.deletes_remote,
            report.errors.len(),
            report.duration_ms,
        );

        if tracing::enabled!(tracing::Level::DEBUG) {
            if let Ok(json) = serde_json::to_string(&report) {
                tracing::debug!("sync_report {json}");
            }
        }

        for ev in &report.http_events {
            tracing::debug!(
                "http {} {} {} {}ms {}B",
                ev.method,
                ev.url,
                ev.status,
                ev.duration_ms,
                ev.bytes
            );
        }

        Ok(report)
    }

    pub fn state(&self) -> Arc<RwLock<SyncState>> {
        self.state.clone()
    }
}

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

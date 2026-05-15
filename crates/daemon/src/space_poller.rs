use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use ocis_client::auth::TokenManager;
use ocis_client::GraphClient;

use crate::config::AppConfig;
use crate::gui_ipc::protocol::DaemonEvent;
use crate::gui_ipc::GuiIpcServer;

pub struct SpacePoller {
    account_id: Uuid,
    config: Arc<Mutex<AppConfig>>,
    config_path: Arc<PathBuf>,
    ipc: Arc<GuiIpcServer>,
    token_manager: Arc<TokenManager>,
    interval: Duration,
    cancel: CancellationToken,
    // tracks space IDs for which SpaceDiscovered has already been emitted this session
    emitted_discoveries: HashSet<String>,
}

impl SpacePoller {
    pub fn new(
        account_id: Uuid,
        config: Arc<Mutex<AppConfig>>,
        config_path: Arc<PathBuf>,
        ipc: Arc<GuiIpcServer>,
        token_manager: Arc<TokenManager>,
        interval: Duration,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            account_id,
            config,
            config_path,
            ipc,
            token_manager,
            interval,
            cancel,
            emitted_discoveries: HashSet::new(),
        }
    }

    pub async fn run(mut self) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.tick().await; // skip first immediate tick
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.poll_once().await;
                }
                _ = self.cancel.cancelled() => break,
            }
        }
    }

    async fn poll_once(&mut self) {
        let (account_url, existing_folders, dismissed) = {
            let cfg = self.config.lock().await;
            let Some(account) = cfg.account.iter().find(|a| a.id == self.account_id) else {
                return;
            };
            (
                account.url.clone(),
                account.folder.clone(),
                account.dismissed_spaces.clone(),
            )
        };

        let base_url = match url::Url::parse(&account_url) {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!(
                    "SpacePoller: invalid URL for account {}: {e}",
                    self.account_id
                );
                return;
            }
        };

        let token_arc = self.token_manager.token_arc();
        let graph = GraphClient::new(base_url, token_arc);

        let remote_spaces = match graph.list_spaces().await {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!(
                    "SpacePoller: list_spaces failed for {}: {e}",
                    self.account_id
                );
                return;
            }
        };

        let remote_ids: HashSet<String> = remote_spaces.iter().map(|s| s.id.clone()).collect();
        let local_ids: HashSet<String> = existing_folders
            .iter()
            .map(|f| f.space_id.clone())
            .collect();
        let dismissed_set: HashSet<String> = dismissed.into_iter().collect();

        // New spaces: in remote, not in local, not dismissed, not already notified
        for space in &remote_spaces {
            if !local_ids.contains(&space.id)
                && !dismissed_set.contains(&space.id)
                && !self.emitted_discoveries.contains(&space.id)
            {
                let suggested = format!(
                    "{}/{}",
                    existing_folders
                        .first()
                        .and_then(|f| std::path::Path::new(&f.local_path).parent())
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| {
                            dirs::home_dir()
                                .map(|h| h.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        }),
                    space.name
                );
                self.ipc.broadcast(DaemonEvent::SpaceDiscovered {
                    account_id: self.account_id,
                    space_id: space.id.clone(),
                    space_name: space.name.clone(),
                    suggested_path: suggested,
                });
                self.emitted_discoveries.insert(space.id.clone());
            }
        }

        // Clear emitted_discoveries for spaces that are now local (user accepted) or dismissed
        self.emitted_discoveries
            .retain(|id| !local_ids.contains(id) && !dismissed_set.contains(id));

        // Removed spaces: in local, not in remote. Re-validate under lock to avoid TOCTOU.
        let removed_folder_ids: HashSet<Uuid> = existing_folders
            .iter()
            .filter(|f| !remote_ids.contains(&f.space_id))
            .map(|f| f.id)
            .collect();

        if !removed_folder_ids.is_empty() {
            let mut cfg = self.config.lock().await;
            if let Some(account) = cfg.account.iter_mut().find(|a| a.id == self.account_id) {
                let to_remove: Vec<_> = account
                    .folder
                    .iter()
                    .filter(|f| removed_folder_ids.contains(&f.id))
                    .cloned()
                    .collect();
                account
                    .folder
                    .retain(|f| !removed_folder_ids.contains(&f.id));
                for folder in &to_remove {
                    self.ipc.broadcast(DaemonEvent::SpaceRemoved {
                        account_id: self.account_id,
                        folder_id: folder.id,
                        space_name: folder.display_name.clone(),
                        local_path: folder.local_path.clone(),
                    });
                }
            }
            if let Err(e) = cfg.save(&self.config_path) {
                tracing::warn!("SpacePoller: failed to save config: {e}");
            }
        }
    }
}

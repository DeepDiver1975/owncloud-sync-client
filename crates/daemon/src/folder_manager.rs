use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use url::Url;
use uuid::Uuid;

use crate::config::{AccountConfig, FolderConfig};
use crate::paths::platform_config_dir;
use crate::vfs_factory::create_vfs;
use crate::watcher::FolderWatcher;
use ocis_client::auth::TokenManager;
use sync_db::SyncJournalDb;
use sync_engine::engine::{EngineConfig, SyncEngine};
use sync_engine::state::SyncState;
use sync_engine::types::ConflictStrategy;
use vfs_core::Vfs;

async fn build_managed_folder(
    fc: &FolderConfig,
    account: &AccountConfig,
    token_manager: Arc<TokenManager>,
    db_dir: &std::path::Path,
) -> Result<ManagedFolder> {
    let root = Utf8PathBuf::from(&fc.local_path);

    let vfs = create_vfs(&fc.vfs_mode, &root)
        .map_err(|e| anyhow::anyhow!("vfs init for folder {}: {e}", fc.id))?;

    let server_url = account.url.trim_end_matches('/');
    let space_root = Url::parse(&format!("{}/dav/spaces/{}/", server_url, fc.space_id))
        .unwrap_or_else(|_| Url::parse("http://localhost/dav/spaces/unknown/").unwrap());

    let db_path = db_dir.join(format!("sync-{}.db", fc.id));
    let db = SyncJournalDb::open(&db_path)
        .await
        .with_context(|| format!("open sync journal for folder {}", fc.id))?;

    let engine = SyncEngine::new(EngineConfig {
        folder_id: fc.id,
        local_root: root.clone(),
        space_root,
        conflict_strategy: ConflictStrategy::KeepBoth,
        max_parallel_transfers: 4,
        db,
        token_manager,
    });

    let watcher = FolderWatcher::watch(root.as_std_path())?;

    Ok(ManagedFolder {
        config: fc.clone(),
        engine: Arc::new(engine),
        vfs,
        watcher: Some(watcher),
    })
}

pub struct ManagedFolder {
    pub config: FolderConfig,
    pub engine: Arc<SyncEngine>,
    pub vfs: Arc<dyn Vfs>,
    pub watcher: Option<FolderWatcher>,
}

pub struct FolderManager {
    pub folders: HashMap<Uuid, ManagedFolder>,
    // Shared map used by SocketApiServer's StatusResolver; engines write into this
    // during sync so the resolver always sees current file statuses.
    sync_states: Arc<RwLock<HashMap<Uuid, SyncState>>>,
}

impl FolderManager {
    pub async fn init_sync(
        folder_configs: &[FolderConfig],
        account_configs: &[AccountConfig],
        token_managers: &std::collections::HashMap<uuid::Uuid, Arc<TokenManager>>,
    ) -> Result<Self> {
        let mut folders = HashMap::new();
        let mut states: HashMap<Uuid, SyncState> = HashMap::new();

        let db_dir = platform_config_dir();
        std::fs::create_dir_all(&db_dir)
            .with_context(|| format!("create config dir {}", db_dir.display()))?;

        for fc in folder_configs {
            let account = account_configs
                .iter()
                .find(|a| a.folder.iter().any(|f| f.id == fc.id));

            let account_ref = match account {
                Some(a) => a,
                None => {
                    tracing::warn!("no account found for folder {}; skipping", fc.id);
                    continue;
                }
            };

            let token_manager = match token_managers.get(&account_ref.id) {
                Some(tm) => Arc::clone(tm),
                None => {
                    tracing::warn!(
                        "no token manager for account {}; skipping folder {}",
                        account_ref.id,
                        fc.id
                    );
                    continue;
                }
            };

            let managed = match build_managed_folder(fc, account_ref, token_manager, &db_dir).await
            {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("failed to set up folder {}: {e}; skipping", fc.id);
                    continue;
                }
            };
            states.insert(fc.id, SyncState::new(fc.id));
            folders.insert(fc.id, managed);
        }

        Ok(Self {
            folders,
            sync_states: Arc::new(RwLock::new(states)),
        })
    }

    /// Register a single new folder at runtime (called after `SetAccountFolder`).
    pub async fn add_folder(
        &mut self,
        fc: &FolderConfig,
        account: &AccountConfig,
        token_manager: Arc<TokenManager>,
    ) -> Result<()> {
        let db_dir = platform_config_dir();
        std::fs::create_dir_all(&db_dir)
            .with_context(|| format!("create config dir {}", db_dir.display()))?;

        let managed = build_managed_folder(fc, account, token_manager, &db_dir).await?;

        {
            let mut states = self.sync_states.write().unwrap_or_else(|e| e.into_inner());
            states.insert(fc.id, SyncState::new(fc.id));
        }

        self.folders.insert(fc.id, managed);
        Ok(())
    }

    /// Take the watcher out of a managed folder (consuming it for use in a forwarding task).
    pub fn take_watcher(&mut self, folder_id: Uuid) -> Option<FolderWatcher> {
        self.folders.get_mut(&folder_id)?.watcher.take()
    }

    pub fn empty() -> Self {
        Self {
            folders: HashMap::new(),
            sync_states: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_engine(&self, id: Uuid) -> Option<&Arc<SyncEngine>> {
        self.folders.get(&id).map(|f| &f.engine)
    }

    /// Returns the shared sync-state map for use by SocketApiServer's StatusResolver.
    pub fn sync_states(&self) -> Arc<RwLock<HashMap<Uuid, SyncState>>> {
        Arc::clone(&self.sync_states)
    }

    /// (local_root, folder_id) pairs for SocketApiServer path dispatch.
    pub fn folder_roots(&self) -> Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>> {
        let pairs: Vec<_> = self
            .folders
            .iter()
            .map(|(id, mf)| (Utf8PathBuf::from(&mf.config.local_path), *id))
            .collect();
        Arc::new(RwLock::new(pairs))
    }

    /// Shared VFS for SocketApiServer (uses first folder's VFS, or VfsOff if none).
    pub fn shared_vfs(&self) -> Arc<dyn Vfs> {
        self.folders
            .values()
            .next()
            .map(|mf| Arc::clone(&mf.vfs))
            .unwrap_or_else(|| Arc::new(vfs_off::VfsOff::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AccountConfig, FolderConfig};
    use ocis_client::auth::oidc::TokenSet;
    use ocis_client::auth::{OidcAuth, TokenManager};
    use tempfile::tempdir;
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_folder_config(local_path: &str) -> FolderConfig {
        FolderConfig {
            id: Uuid::new_v4(),
            local_path: local_path.to_string(),
            space_id: "test-space".to_string(),
            display_name: "Test Folder".to_string(),
            selective_sync_excluded: vec![],
            vfs_mode: "off".to_string(),
            paused: false,
        }
    }

    fn make_account_config(folders: Vec<FolderConfig>) -> AccountConfig {
        AccountConfig {
            id: Uuid::new_v4(),
            url: "https://ocis.example.com".to_string(),
            user_id: String::new(),
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            folder: folders,
            dismissed_spaces: vec![],
        }
    }

    async fn make_token_manager(account_id: Uuid) -> (MockServer, Arc<TokenManager>) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                r#"{{
                    "issuer": "{uri}",
                    "authorization_endpoint": "{uri}/auth",
                    "token_endpoint": "{uri}/token"
                }}"#,
                uri = server.uri()
            )))
            .mount(&server)
            .await;

        let oidc = OidcAuth::discover(
            &server.uri(),
            "test-client",
            None,
            "http://localhost:9999/callback",
            false,
        )
        .await
        .unwrap();

        let token = TokenSet {
            access_token: "test".into(),
            refresh_token: None,
            expires_at: i64::MAX,
        };
        let tm = Arc::new(TokenManager::new(oidc, token, account_id.to_string()));
        (server, tm)
    }

    #[tokio::test]
    async fn init_two_folders() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();

        let fc1 = make_folder_config(dir1.path().to_str().unwrap());
        let fc2 = make_folder_config(dir2.path().to_str().unwrap());

        let account = make_account_config(vec![fc1.clone(), fc2.clone()]);

        let (_server, tm) = make_token_manager(account.id).await;
        let mut token_managers = std::collections::HashMap::new();
        token_managers.insert(account.id, tm);

        let fm = FolderManager::init_sync(&[fc1.clone(), fc2.clone()], &[account], &token_managers)
            .await
            .unwrap();

        assert_eq!(fm.folders.len(), 2);

        let map = fm.sync_states();
        let map = map.read().unwrap();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&fc1.id));
        assert!(map.contains_key(&fc2.id));
    }

    #[test]
    fn empty_has_no_folders() {
        let fm = FolderManager::empty();
        assert!(fm.folders.is_empty());
    }
}

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use uuid::Uuid;

use ocis_client::auth::{OidcAuth, TokenManager};
use ocis_client::GraphClient;

use super::protocol::{DaemonCommand, DaemonEvent, SpaceInfo};
use super::GuiIpcServer;
use crate::config::{AppConfig, FolderConfig};
use crate::folder_manager::FolderManager;
use crate::oidc_callback;
use crate::scheduler::SyncScheduler;

pub(crate) const OCIS_CLIENT_ID: &str =
    "xdXOt13JKxym1B1QcEncf2XDkLAexMBFwiT9j6EfhhHFJhs2KM9jbjTmf8JBXE69";
pub(crate) const OCIS_CLIENT_SECRET: &str =
    "UBntmLjC2yYCeHwsyj73Uwo9TAaecAetRwMw0xYcvNL9yRdLSUi0hUAHfvCHFeFh";

#[derive(Debug, PartialEq)]
pub enum ShouldQuit {
    Yes,
    No,
}

pub struct HandleContext<'a> {
    pub scheduler: Arc<Mutex<SyncScheduler>>,
    pub folder_manager: &'a mut FolderManager,
    pub ipc: Arc<GuiIpcServer>,
    pub config: Arc<Mutex<AppConfig>>,
    pub config_path: PathBuf,
    pub live_folder_ids: Arc<std::sync::RwLock<Vec<Uuid>>>,
    pub token_managers: Arc<std::sync::RwLock<std::collections::HashMap<Uuid, Arc<TokenManager>>>>,
    pub watcher_tx: tokio::sync::mpsc::Sender<(Uuid, notify::Event)>,
}

pub async fn handle_command(cmd: DaemonCommand, ctx: &mut HandleContext<'_>) -> Result<ShouldQuit> {
    let HandleContext {
        scheduler,
        folder_manager,
        ipc,
        config,
        config_path,
        live_folder_ids,
        token_managers,
        watcher_tx,
    } = ctx;

    match cmd {
        DaemonCommand::Subscribe => {}

        DaemonCommand::TriggerSync { folder_id } => {
            scheduler.lock().await.force_request_sync(folder_id);
            ipc.broadcast(DaemonEvent::SyncStarted { folder_id });
        }

        DaemonCommand::PauseFolder { folder_id } => {
            scheduler.lock().await.pause(folder_id);
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id: folder_id,
                state: "paused".into(),
            });
        }

        DaemonCommand::ResumeFolder { folder_id } => {
            scheduler.lock().await.resume(folder_id);
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id: folder_id,
                state: "active".into(),
            });
        }

        DaemonCommand::AddAccount { url } => {
            let account_id = Uuid::new_v4();

            let url = format!("https://{url}");

            let insecure = config.lock().await.general.insecure;

            let listener = match TcpListener::bind("127.0.0.1:0").await {
                Ok(l) => l,
                Err(e) => {
                    ipc.broadcast(DaemonEvent::AccountAddFailed {
                        account_id,
                        reason: format!("failed to bind callback port: {e}"),
                    });
                    return Ok(ShouldQuit::No);
                }
            };
            let port = listener.local_addr()?.port();
            let callback_uri = format!("http://127.0.0.1:{port}/callback");

            ipc.broadcast(DaemonEvent::AccountAddStarted { account_id });

            let ipc_clone = Arc::clone(ipc);
            let config_clone = Arc::clone(config);
            let config_path_clone = config_path.clone();
            let token_managers_clone = Arc::clone(token_managers);
            tokio::spawn(async move {
                let oidc = match OidcAuth::discover(
                    &url,
                    OCIS_CLIENT_ID,
                    Some(OCIS_CLIENT_SECRET.to_string()),
                    &callback_uri,
                    insecure,
                )
                .await
                {
                    Ok(o) => o,
                    Err(e) => {
                        ipc_clone.broadcast(DaemonEvent::AccountAddFailed {
                            account_id,
                            reason: format!("OIDC discovery failed: {e}"),
                        });
                        return;
                    }
                };

                let (auth_url, verifier) = match oidc.start_pkce_flow() {
                    Ok(pair) => pair,
                    Err(e) => {
                        ipc_clone.broadcast(DaemonEvent::AccountAddFailed {
                            account_id,
                            reason: format!("PKCE setup failed: {e}"),
                        });
                        return;
                    }
                };

                println!("OIDC_AUTH_URL={}", auth_url);

                let no_browser = std::env::var("OCSYNCD_NO_BROWSER").is_ok();
                if !no_browser {
                    let auth_url_str = auth_url.to_string();
                    tokio::spawn(async move {
                        if let Err(e) = open_browser(&auth_url_str).await {
                            tracing::warn!("could not open browser: {e}");
                        }
                    });
                }

                oidc_callback::run_callback(
                    listener,
                    oidc,
                    verifier,
                    account_id,
                    url,
                    ipc_clone,
                    config_clone,
                    config_path_clone,
                    token_managers_clone,
                )
                .await;
            });
        }

        DaemonCommand::RemoveAccount { account_id } => {
            let mut cfg = config.lock().await;
            cfg.account.retain(|a| a.id != account_id);
            cfg.save(config_path)?;
            drop(cfg);
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id,
                state: "removed".into(),
            });
        }

        DaemonCommand::ListSpaces { account_id } => {
            let account_url = {
                let cfg = config.lock().await;
                match cfg.account.iter().find(|a| a.id == account_id) {
                    Some(a) => a.url.clone(),
                    None => {
                        ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                            account_id,
                            reason: "account not found".into(),
                        });
                        return Ok(ShouldQuit::No);
                    }
                }
            };

            let tm = token_managers.read().unwrap().get(&account_id).cloned();
            let token_manager = match tm {
                Some(tm) => tm,
                None => {
                    ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                        account_id,
                        reason: "no credentials found for account".into(),
                    });
                    return Ok(ShouldQuit::No);
                }
            };

            let ipc_clone = Arc::clone(ipc);
            tokio::spawn(async move {
                let base_url = match url::Url::parse(&account_url) {
                    Ok(u) => u,
                    Err(e) => {
                        ipc_clone.broadcast(DaemonEvent::AccountSpaceFailed {
                            account_id,
                            reason: format!("invalid server URL: {e}"),
                        });
                        return;
                    }
                };
                let token_arc = token_manager.token_arc();
                let graph = GraphClient::new(base_url, token_arc);
                match graph.list_spaces().await {
                    Ok(spaces) => {
                        ipc_clone.broadcast(DaemonEvent::SpacesListed {
                            account_id,
                            spaces: spaces
                                .into_iter()
                                .map(|s| SpaceInfo {
                                    id: s.id,
                                    name: s.name,
                                    drive_type: s.drive_type,
                                })
                                .collect(),
                        });
                    }
                    Err(e) => {
                        ipc_clone.broadcast(DaemonEvent::AccountSpaceFailed {
                            account_id,
                            reason: format!("failed to list spaces: {e}"),
                        });
                    }
                }
            });
        }

        DaemonCommand::SetAccountFolders {
            account_id,
            root_path,
            spaces,
        } => {
            let account_exists = {
                let cfg = config.lock().await;
                cfg.account.iter().any(|a| a.id == account_id)
            };
            if !account_exists {
                ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                    account_id,
                    reason: "account not found".into(),
                });
                return Ok(ShouldQuit::No);
            }

            let tm = token_managers.read().unwrap().get(&account_id).cloned();
            let token_manager = match tm {
                Some(tm) => tm,
                None => {
                    ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                        account_id,
                        reason: "no credentials found for account".into(),
                    });
                    return Ok(ShouldQuit::No);
                }
            };

            for space_sel in spaces {
                let sub_path = std::path::Path::new(&root_path).join(&space_sel.display_name);
                if let Err(e) = tokio::fs::create_dir_all(&sub_path).await {
                    ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                        account_id,
                        reason: format!("failed to create folder '{}': {e}", sub_path.display()),
                    });
                    continue;
                }

                let folder_id = Uuid::new_v4();
                let local_path = sub_path.to_string_lossy().into_owned();
                let new_folder = FolderConfig {
                    id: folder_id,
                    local_path: local_path.clone(),
                    space_id: space_sel.space_id.clone(),
                    display_name: space_sel.display_name.clone(),
                    selective_sync_excluded: vec![],
                    vfs_mode: "off".to_string(),
                    paused: false,
                };

                let account_snapshot = {
                    let cfg = config.lock().await;
                    cfg.account.iter().find(|a| a.id == account_id).cloned()
                };

                if let Some(account) = account_snapshot {
                    match folder_manager
                        .add_folder(&new_folder, &account, Arc::clone(&token_manager))
                        .await
                    {
                        Ok(_) => {
                            {
                                let mut cfg = config.lock().await;
                                if let Some(acc) =
                                    cfg.account.iter_mut().find(|a| a.id == account_id)
                                {
                                    acc.folder.push(new_folder.clone());
                                }
                                if let Err(e) = cfg.save(config_path) {
                                    tracing::warn!("failed to save config: {e}");
                                }
                            }
                            {
                                let mut sched = scheduler.lock().await;
                                sched.add_folder(folder_id);
                                sched.request_sync(folder_id);
                            }
                            live_folder_ids.write().unwrap().push(folder_id);
                            if let Some(mut watcher) = folder_manager.take_watcher(folder_id) {
                                let tx = watcher_tx.clone();
                                tokio::spawn(async move {
                                    while let Some(event) = watcher.next_event().await {
                                        let _ = tx.send((folder_id, event)).await;
                                    }
                                });
                            }
                            ipc.broadcast(DaemonEvent::AccountFolderAdded {
                                account_id,
                                folder_id,
                                space_id: space_sel.space_id.clone(),
                                local_path,
                                display_name: space_sel.display_name,
                            });
                        }
                        Err(e) => {
                            tracing::warn!("failed to register folder with engine: {e}");
                            ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                                account_id,
                                reason: format!("failed to register folder: {e}"),
                            });
                        }
                    }
                }
            }
        }

        DaemonCommand::AddAccountSpace {
            account_id,
            space_id,
            local_path,
        } => {
            let account_exists = {
                let cfg = config.lock().await;
                cfg.account.iter().any(|a| a.id == account_id)
            };
            if !account_exists {
                ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                    account_id,
                    reason: "account not found".into(),
                });
                return Ok(ShouldQuit::No);
            }

            let tm = token_managers.read().unwrap().get(&account_id).cloned();
            let token_manager = match tm {
                Some(tm) => tm,
                None => {
                    ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                        account_id,
                        reason: "no credentials found for account".into(),
                    });
                    return Ok(ShouldQuit::No);
                }
            };

            if let Err(e) = tokio::fs::create_dir_all(&local_path).await {
                ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                    account_id,
                    reason: format!("failed to create folder '{local_path}': {e}"),
                });
                return Ok(ShouldQuit::No);
            }

            let display_name = std::path::Path::new(&local_path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| space_id.clone());

            let folder_id = Uuid::new_v4();
            let new_folder = FolderConfig {
                id: folder_id,
                local_path: local_path.clone(),
                space_id: space_id.clone(),
                display_name: display_name.clone(),
                selective_sync_excluded: vec![],
                vfs_mode: "off".to_string(),
                paused: false,
            };

            let account_snapshot = {
                let cfg = config.lock().await;
                cfg.account.iter().find(|a| a.id == account_id).cloned()
            };

            if let Some(account) = account_snapshot {
                match folder_manager
                    .add_folder(&new_folder, &account, Arc::clone(&token_manager))
                    .await
                {
                    Ok(_) => {
                        {
                            let mut cfg = config.lock().await;
                            if let Some(acc) = cfg.account.iter_mut().find(|a| a.id == account_id) {
                                acc.folder.push(new_folder.clone());
                            }
                            if let Err(e) = cfg.save(config_path) {
                                tracing::warn!("failed to save config: {e}");
                            }
                        }
                        {
                            let mut sched = scheduler.lock().await;
                            sched.add_folder(folder_id);
                            sched.request_sync(folder_id);
                        }
                        live_folder_ids.write().unwrap().push(folder_id);
                        if let Some(mut watcher) = folder_manager.take_watcher(folder_id) {
                            let tx = watcher_tx.clone();
                            tokio::spawn(async move {
                                while let Some(event) = watcher.next_event().await {
                                    let _ = tx.send((folder_id, event)).await;
                                }
                            });
                        }
                        ipc.broadcast(DaemonEvent::AccountFolderAdded {
                            account_id,
                            folder_id,
                            space_id: space_id.clone(),
                            local_path,
                            display_name,
                        });
                    }
                    Err(e) => {
                        tracing::warn!("failed to register folder: {e}");
                        ipc.broadcast(DaemonEvent::AccountSpaceFailed {
                            account_id,
                            reason: format!("failed to register folder: {e}"),
                        });
                    }
                }
            }
        }

        DaemonCommand::DismissSpace {
            account_id,
            space_id,
        } => {
            let mut cfg = config.lock().await;
            if let Some(account) = cfg.account.iter_mut().find(|a| a.id == account_id) {
                if !account.dismissed_spaces.contains(&space_id) {
                    account.dismissed_spaces.push(space_id);
                }
                if let Err(e) = cfg.save(config_path) {
                    tracing::warn!("failed to save config after dismiss: {e}");
                }
            }
        }

        DaemonCommand::Quit => {
            return Ok(ShouldQuit::Yes);
        }
    }
    Ok(ShouldQuit::No)
}

pub(crate) async fn run_browser_cmd(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = tokio::process::Command::new(cmd)
        .args(args)
        .status()
        .await
        .map_err(|e| anyhow::anyhow!("failed to start '{cmd}': {e}"))?;
    if !status.success() {
        anyhow::bail!("'{cmd}' exited with {}", status);
    }
    Ok(())
}

async fn open_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    return run_browser_cmd("xdg-open", &[url]).await;
    #[cfg(target_os = "macos")]
    return run_browser_cmd("open", &[url]).await;
    #[cfg(target_os = "windows")]
    return run_browser_cmd("cmd", &["/c", "start", url]).await;
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    anyhow::bail!("unsupported platform for browser launch");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GeneralConfig;
    use crate::folder_manager::FolderManager;
    use crate::scheduler::SyncScheduler;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn trigger_sync_marks_pending() {
        let folder_id = Uuid::new_v4();
        let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![folder_id])));
        let (ipc, _rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        let (watcher_tx, _watcher_rx) = tokio::sync::mpsc::channel::<(Uuid, notify::Event)>(1);
        let result = handle_command(
            DaemonCommand::TriggerSync { folder_id },
            &mut HandleContext {
                scheduler: Arc::clone(&scheduler),
                folder_manager: &mut fm,
                ipc,
                config,
                config_path: file.path().to_path_buf(),
                live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
                token_managers: Arc::new(std::sync::RwLock::new(std::collections::HashMap::<
                    Uuid,
                    Arc<TokenManager>,
                >::new())),
                watcher_tx,
            },
        )
        .await
        .unwrap();

        assert_eq!(result, ShouldQuit::No);
    }

    #[tokio::test]
    async fn pause_marks_paused() {
        let folder_id = Uuid::new_v4();
        let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![folder_id])));
        let (ipc, _rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        let (watcher_tx, _watcher_rx) = tokio::sync::mpsc::channel::<(Uuid, notify::Event)>(1);
        handle_command(
            DaemonCommand::PauseFolder { folder_id },
            &mut HandleContext {
                scheduler: Arc::clone(&scheduler),
                folder_manager: &mut fm,
                ipc,
                config,
                config_path: file.path().to_path_buf(),
                live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
                token_managers: Arc::new(std::sync::RwLock::new(std::collections::HashMap::<
                    Uuid,
                    Arc<TokenManager>,
                >::new())),
                watcher_tx,
            },
        )
        .await
        .unwrap();

        let mut sched = scheduler.lock().await;
        sched.request_sync(folder_id);
        assert!(!sched.ready_to_run().contains(&folder_id));
    }

    #[tokio::test]
    async fn resume_unpauses() {
        let folder_id = Uuid::new_v4();
        let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![folder_id])));
        let (ipc, _rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        let (watcher_tx1, _watcher_rx1) = tokio::sync::mpsc::channel::<(Uuid, notify::Event)>(1);
        handle_command(
            DaemonCommand::PauseFolder { folder_id },
            &mut HandleContext {
                scheduler: Arc::clone(&scheduler),
                folder_manager: &mut fm,
                ipc: Arc::clone(&ipc),
                config: Arc::clone(&config),
                config_path: file.path().to_path_buf(),
                live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
                token_managers: Arc::new(std::sync::RwLock::new(std::collections::HashMap::<
                    Uuid,
                    Arc<TokenManager>,
                >::new())),
                watcher_tx: watcher_tx1,
            },
        )
        .await
        .unwrap();
        let (watcher_tx2, _watcher_rx2) = tokio::sync::mpsc::channel::<(Uuid, notify::Event)>(1);
        handle_command(
            DaemonCommand::ResumeFolder { folder_id },
            &mut HandleContext {
                scheduler: Arc::clone(&scheduler),
                folder_manager: &mut fm,
                ipc,
                config,
                config_path: file.path().to_path_buf(),
                live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
                token_managers: Arc::new(std::sync::RwLock::new(std::collections::HashMap::<
                    Uuid,
                    Arc<TokenManager>,
                >::new())),
                watcher_tx: watcher_tx2,
            },
        )
        .await
        .unwrap();
        let mut sched = scheduler.lock().await;
        sched.request_sync(folder_id);
        assert!(sched.ready_to_run().contains(&folder_id));
    }

    #[tokio::test]
    async fn quit_returns_should_quit() {
        let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
        let (ipc, _rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        let (watcher_tx, _watcher_rx) = tokio::sync::mpsc::channel::<(Uuid, notify::Event)>(1);
        let result = handle_command(
            DaemonCommand::Quit,
            &mut HandleContext {
                scheduler: Arc::clone(&scheduler),
                folder_manager: &mut fm,
                ipc,
                config,
                config_path: file.path().to_path_buf(),
                live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
                token_managers: Arc::new(std::sync::RwLock::new(std::collections::HashMap::<
                    Uuid,
                    Arc<TokenManager>,
                >::new())),
                watcher_tx,
            },
        )
        .await
        .unwrap();

        assert_eq!(result, ShouldQuit::Yes);
    }

    #[tokio::test]
    async fn run_browser_cmd_returns_err_if_command_not_found() {
        let result = run_browser_cmd(
            "this-binary-definitely-does-not-exist-xyz",
            &["http://example.com"],
        )
        .await;
        assert!(result.is_err(), "expected Err for missing command");
    }

    #[tokio::test]
    async fn run_browser_cmd_returns_err_if_exit_nonzero() {
        let result = run_browser_cmd("sh", &["-c", "exit 1"]).await;
        assert!(result.is_err(), "expected Err for non-zero exit");
    }

    #[tokio::test]
    async fn add_account_oidc_failure_broadcasts_account_add_failed() {
        let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
        let (ipc, mut rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        let (watcher_tx, _watcher_rx) = tokio::sync::mpsc::channel::<(Uuid, notify::Event)>(1);
        // OIDC discovery against a non-existent server must broadcast AccountAddFailed.
        let result = handle_command(
            DaemonCommand::AddAccount {
                url: "cloud.example.com".to_string(),
            },
            &mut HandleContext {
                scheduler: Arc::clone(&scheduler),
                folder_manager: &mut fm,
                ipc,
                config,
                config_path: file.path().to_path_buf(),
                live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
                token_managers: Arc::new(std::sync::RwLock::new(std::collections::HashMap::<
                    Uuid,
                    Arc<TokenManager>,
                >::new())),
                watcher_tx,
            },
        )
        .await
        .unwrap();

        assert_eq!(result, ShouldQuit::No);

        // AccountAddStarted is now emitted immediately before OIDC discovery.
        let first = rx.try_recv().expect("expected AccountAddStarted");
        assert!(
            matches!(first, DaemonEvent::AccountAddStarted { .. }),
            "expected AccountAddStarted, got {first:?}"
        );

        // AccountAddFailed arrives once the background task completes OIDC discovery.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let failed = loop {
            if let Ok(event) = rx.try_recv() {
                break event;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("timed out waiting for AccountAddFailed");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        assert!(
            matches!(failed, DaemonEvent::AccountAddFailed { .. }),
            "expected AccountAddFailed, got {failed:?}"
        );
    }
}

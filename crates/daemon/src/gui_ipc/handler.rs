use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use ocis_client::auth::{KeychainStore, OidcAuth};
use ocis_client::GraphClient;

use super::protocol::{DaemonCommand, DaemonEvent};
use super::GuiIpcServer;
use crate::config::{AppConfig, FolderConfig};
use crate::folder_manager::FolderManager;
use crate::oidc_callback;
use crate::scheduler::SyncScheduler;

const OCIS_CLIENT_ID: &str = "xdXOt13JKxym1B1QcEncf2XDkLAexMBFwiT9j6EfhhHFJhs2KM9jbjTmf8JBXE69";
const OCIS_CLIENT_SECRET: &str = "UBntmLjC2yYCeHwsyj73Uwo9TAaecAetRwMw0xYcvNL9yRdLSUi0hUAHfvCHFeFh";

#[derive(Debug, PartialEq)]
pub enum ShouldQuit {
    Yes,
    No,
}

pub async fn handle_command(
    cmd: DaemonCommand,
    scheduler: &mut SyncScheduler,
    folder_manager: &mut FolderManager,
    ipc: &Arc<GuiIpcServer>,
    config: Arc<Mutex<AppConfig>>,
    config_path: PathBuf,
    live_folder_ids: Arc<std::sync::RwLock<Vec<Uuid>>>,
) -> Result<ShouldQuit> {
    match cmd {
        DaemonCommand::Subscribe => {}

        DaemonCommand::TriggerSync { folder_id } => {
            scheduler.force_request_sync(folder_id);
            ipc.broadcast(DaemonEvent::SyncStarted { folder_id });
        }

        DaemonCommand::PauseFolder { folder_id } => {
            scheduler.pause(folder_id);
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id: folder_id,
                state: "paused".into(),
            });
        }

        DaemonCommand::ResumeFolder { folder_id } => {
            scheduler.resume(folder_id);
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id: folder_id,
                state: "active".into(),
            });
        }

        DaemonCommand::AddAccount { url } => {
            let account_id = Uuid::new_v4();

            if !url.starts_with("http://") && !url.starts_with("https://") {
                ipc.broadcast(DaemonEvent::AccountAddFailed {
                    account_id,
                    reason: "URL must start with http:// or https://".to_string(),
                });
                return Ok(ShouldQuit::No);
            }

            let insecure = config.lock().await.general.insecure;

            // Bind to :0 to get an OS-assigned port.
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

            // Emit AccountAddStarted immediately so clients don't wait on OIDC discovery.
            ipc.broadcast(DaemonEvent::AccountAddStarted { account_id });

            let ipc_clone = Arc::clone(ipc);
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
                    config,
                    config_path,
                )
                .await;
            });
        }

        DaemonCommand::RemoveAccount { account_id } => {
            let mut cfg = config.lock().await;
            cfg.account.retain(|a| a.id != account_id);
            cfg.save(&config_path)?;
            drop(cfg);
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id,
                state: "removed".into(),
            });
        }

        DaemonCommand::SetAccountFolder {
            account_id,
            local_path,
        } => {
            // Step 1: Find the account in config.
            let (account_url, account_id_str) = {
                let cfg = config.lock().await;
                match cfg.account.iter().find(|a| a.id == account_id) {
                    Some(a) => (a.url.clone(), a.id.to_string()),
                    None => {
                        ipc.broadcast(DaemonEvent::AccountSetFolderFailed {
                            account_id,
                            reason: "account not found".to_string(),
                        });
                        return Ok(ShouldQuit::No);
                    }
                }
            };

            // Step 2: Validate local_path exists and is a directory.
            let path = std::path::Path::new(&local_path);
            if !path.exists() || !path.is_dir() {
                ipc.broadcast(DaemonEvent::AccountSetFolderFailed {
                    account_id,
                    reason: "path does not exist or is not a directory".to_string(),
                });
                return Ok(ShouldQuit::No);
            }

            // Step 3: Load keychain tokens.
            let key = account_id_str.clone();
            let token_set = match tokio::task::spawn_blocking(move || KeychainStore::load(&key))
                .await
                .map_err(|e| anyhow::anyhow!("keychain task panicked: {e}"))?
            {
                Ok(Some(t)) => t,
                Ok(None) => {
                    ipc.broadcast(DaemonEvent::AccountSetFolderFailed {
                        account_id,
                        reason: "could not load account credentials".to_string(),
                    });
                    return Ok(ShouldQuit::No);
                }
                Err(e) => {
                    ipc.broadcast(DaemonEvent::AccountSetFolderFailed {
                        account_id,
                        reason: format!("could not load account credentials: {e}"),
                    });
                    return Ok(ShouldQuit::No);
                }
            };

            // Step 4: Construct GraphClient.
            let base_url = match url::Url::parse(&account_url) {
                Ok(u) => u,
                Err(e) => {
                    ipc.broadcast(DaemonEvent::AccountSetFolderFailed {
                        account_id,
                        reason: format!("invalid server URL: {e}"),
                    });
                    return Ok(ShouldQuit::No);
                }
            };
            let token_arc = Arc::new(RwLock::new(token_set));
            let graph = GraphClient::new(base_url, token_arc);

            // Step 5: List spaces and find the personal drive.
            let spaces = match graph.list_spaces().await {
                Ok(s) => s,
                Err(e) => {
                    ipc.broadcast(DaemonEvent::AccountSetFolderFailed {
                        account_id,
                        reason: format!("failed to list spaces: {e}"),
                    });
                    return Ok(ShouldQuit::No);
                }
            };
            let personal = match spaces.into_iter().find(|s| s.drive_type == "personal") {
                Some(s) => s,
                None => {
                    ipc.broadcast(DaemonEvent::AccountSetFolderFailed {
                        account_id,
                        reason: "no personal drive found".to_string(),
                    });
                    return Ok(ShouldQuit::No);
                }
            };

            // Step 6 & 7: Push FolderConfig, save, register with engine, and broadcast.
            let folder_id = Uuid::new_v4();
            let new_folder_config = FolderConfig {
                id: folder_id,
                local_path: local_path.clone(),
                space_id: personal.id,
                display_name: "Personal".to_string(),
                selective_sync_excluded: vec![],
                vfs_mode: "off".to_string(),
                paused: false,
            };
            let account_config = {
                let mut cfg = config.lock().await;
                if let Some(account) = cfg.account.iter_mut().find(|a| a.id == account_id) {
                    account.folder.push(new_folder_config.clone());
                }
                cfg.save(&config_path)?;
                cfg.account.iter().find(|a| a.id == account_id).cloned()
            };

            // Register the new folder with the engine and scheduler so sync runs immediately.
            if let Some(account) = account_config {
                if let Err(e) = folder_manager
                    .add_folder(&new_folder_config, &account)
                    .await
                {
                    tracing::warn!("failed to register folder with engine: {e}");
                } else {
                    scheduler.add_folder(folder_id);
                    live_folder_ids.write().unwrap().push(folder_id);
                    scheduler.request_sync(folder_id);
                }
            }

            ipc.broadcast(DaemonEvent::AccountFolderAdded {
                account_id,
                folder_id,
                local_path,
                display_name: "Personal".to_string(),
            });
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
        let mut scheduler = SyncScheduler::new(vec![folder_id]);
        let (ipc, _rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        let result = handle_command(
            DaemonCommand::TriggerSync { folder_id },
            &mut scheduler,
            &mut fm,
            &ipc,
            config,
            file.path().to_path_buf(),
            Arc::new(std::sync::RwLock::new(vec![])),
        )
        .await
        .unwrap();

        assert_eq!(result, ShouldQuit::No);
    }

    #[tokio::test]
    async fn pause_marks_paused() {
        let folder_id = Uuid::new_v4();
        let mut scheduler = SyncScheduler::new(vec![folder_id]);
        let (ipc, _rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        handle_command(
            DaemonCommand::PauseFolder { folder_id },
            &mut scheduler,
            &mut fm,
            &ipc,
            config,
            file.path().to_path_buf(),
            Arc::new(std::sync::RwLock::new(vec![])),
        )
        .await
        .unwrap();

        scheduler.request_sync(folder_id);
        assert!(!scheduler.ready_to_run().contains(&folder_id));
    }

    #[tokio::test]
    async fn resume_unpauses() {
        let folder_id = Uuid::new_v4();
        let mut scheduler = SyncScheduler::new(vec![folder_id]);
        let (ipc, _rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        handle_command(
            DaemonCommand::PauseFolder { folder_id },
            &mut scheduler,
            &mut fm,
            &ipc,
            Arc::clone(&config),
            file.path().to_path_buf(),
            Arc::new(std::sync::RwLock::new(vec![])),
        )
        .await
        .unwrap();
        handle_command(
            DaemonCommand::ResumeFolder { folder_id },
            &mut scheduler,
            &mut fm,
            &ipc,
            config,
            file.path().to_path_buf(),
            Arc::new(std::sync::RwLock::new(vec![])),
        )
        .await
        .unwrap();
        scheduler.request_sync(folder_id);
        assert!(scheduler.ready_to_run().contains(&folder_id));
    }

    #[tokio::test]
    async fn quit_returns_should_quit() {
        let mut scheduler = SyncScheduler::new(vec![]);
        let (ipc, _rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        let result = handle_command(
            DaemonCommand::Quit,
            &mut scheduler,
            &mut fm,
            &ipc,
            config,
            file.path().to_path_buf(),
            Arc::new(std::sync::RwLock::new(vec![])),
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
        let mut scheduler = SyncScheduler::new(vec![]);
        let (ipc, mut rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();

        // OIDC discovery against a non-existent server must broadcast AccountAddFailed.
        let result = handle_command(
            DaemonCommand::AddAccount {
                url: "https://cloud.example.com".to_string(),
            },
            &mut scheduler,
            &mut fm,
            &ipc,
            config,
            file.path().to_path_buf(),
            Arc::new(std::sync::RwLock::new(vec![])),
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

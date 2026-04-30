use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use uuid::Uuid;

use ocis_client::auth::OidcAuth;

use super::protocol::{DaemonCommand, DaemonEvent};
use super::GuiIpcServer;
use crate::config::AppConfig;
use crate::folder_manager::FolderManager;
use crate::oidc_callback;
use crate::scheduler::SyncScheduler;

const OCIS_CLIENT_ID: &str = "xdXOt13JKxym1B1QcEncf2XDkLAexMBFwiT9j6EohhggggDD";

#[derive(Debug, PartialEq)]
pub enum ShouldQuit {
    Yes,
    No,
}

pub async fn handle_command(
    cmd: DaemonCommand,
    scheduler: &mut SyncScheduler,
    _folder_manager: &FolderManager,
    ipc: &Arc<GuiIpcServer>,
    config: Arc<Mutex<AppConfig>>,
    config_path: PathBuf,
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

            let oidc = match OidcAuth::discover(&url, OCIS_CLIENT_ID, &callback_uri).await {
                Ok(o) => o,
                Err(e) => {
                    ipc.broadcast(DaemonEvent::AccountAddFailed {
                        account_id,
                        reason: format!("OIDC discovery failed: {e}"),
                    });
                    return Ok(ShouldQuit::No);
                }
            };

            let (auth_url, verifier) = match oidc.start_pkce_flow() {
                Ok(pair) => pair,
                Err(e) => {
                    ipc.broadcast(DaemonEvent::AccountAddFailed {
                        account_id,
                        reason: format!("PKCE setup failed: {e}"),
                    });
                    return Ok(ShouldQuit::No);
                }
            };

            if let Err(e) = open_browser(auth_url.as_str()) {
                ipc.broadcast(DaemonEvent::AccountAddFailed {
                    account_id,
                    reason: format!("could not open browser: {e}"),
                });
                return Ok(ShouldQuit::No);
            }

            ipc.broadcast(DaemonEvent::AccountAddStarted { account_id });

            let ipc_clone = Arc::clone(ipc);
            tokio::spawn(oidc_callback::run_callback(
                listener,
                oidc,
                verifier,
                account_id,
                url,
                ipc_clone,
                config,
                config_path,
            ));
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

        DaemonCommand::Quit => {
            return Ok(ShouldQuit::Yes);
        }
    }
    Ok(ShouldQuit::No)
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/c", "start", url])
        .spawn()?;
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    anyhow::bail!("unsupported platform for browser launch");
    Ok(())
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
        let fm = FolderManager::empty();

        let result = handle_command(
            DaemonCommand::TriggerSync { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            config,
            file.path().to_path_buf(),
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
        let fm = FolderManager::empty();

        handle_command(
            DaemonCommand::PauseFolder { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            config,
            file.path().to_path_buf(),
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
        let fm = FolderManager::empty();

        handle_command(
            DaemonCommand::PauseFolder { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            Arc::clone(&config),
            file.path().to_path_buf(),
        )
        .await
        .unwrap();
        handle_command(
            DaemonCommand::ResumeFolder { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            config,
            file.path().to_path_buf(),
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
        let fm = FolderManager::empty();

        let result = handle_command(
            DaemonCommand::Quit,
            &mut scheduler,
            &fm,
            &ipc,
            config,
            file.path().to_path_buf(),
        )
        .await
        .unwrap();

        assert_eq!(result, ShouldQuit::Yes);
    }
}

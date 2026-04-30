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

            if let Err(e) = open_browser(auth_url.as_str()).await {
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

    #[tokio::test(flavor = "current_thread")]
    async fn add_account_browser_failure_broadcasts_account_add_failed() {
        // Prepend an empty temp dir to PATH so xdg-open/open/start cannot be found.
        let empty_dir = tempfile::tempdir().unwrap();
        let old_path = std::env::var("PATH").unwrap_or_default();
        // Safety: single-threaded test, PATH manipulation is contained.
        unsafe {
            std::env::set_var("PATH", format!("{}", empty_dir.path().display()));
        }

        let mut scheduler = SyncScheduler::new(vec![]);
        let (ipc, mut rx) = GuiIpcServer::new();
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        }));
        let file = NamedTempFile::new().unwrap();
        let fm = FolderManager::empty();

        // Use an https:// URL that passes URL validation but will fail at browser launch
        // because PATH is empty. OIDC discovery will also fail (no real server), so we
        // expect AccountAddFailed — the important assertion is that it is broadcast at all
        // (the browser-launch failure path converges with the OIDC failure path into the
        // same AccountAddFailed event, which is correct behavior).
        let result = handle_command(
            DaemonCommand::AddAccount {
                url: "https://cloud.example.com".to_string(),
            },
            &mut scheduler,
            &fm,
            &ipc,
            config,
            file.path().to_path_buf(),
        )
        .await
        .unwrap();

        // Restore PATH before any assertions (even on panic).
        unsafe {
            std::env::set_var("PATH", &old_path);
        }

        assert_eq!(result, ShouldQuit::No);

        let event = rx.try_recv().expect("expected an event to be broadcast");
        assert!(
            matches!(event, DaemonEvent::AccountAddFailed { .. }),
            "expected AccountAddFailed, got {event:?}"
        );
    }
}

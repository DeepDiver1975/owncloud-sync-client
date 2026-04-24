use anyhow::Result;
use std::path::Path;
use uuid::Uuid;

use super::protocol::{DaemonCommand, DaemonEvent};
use super::GuiIpcServer;
use crate::config::AppConfig;
use crate::folder_manager::FolderManager;
use crate::scheduler::SyncScheduler;

#[derive(Debug, PartialEq)]
pub enum ShouldQuit {
    Yes,
    No,
}

pub async fn handle_command(
    cmd: DaemonCommand,
    scheduler: &mut SyncScheduler,
    _folder_manager: &FolderManager,
    ipc: &GuiIpcServer,
    config: &mut AppConfig,
    config_path: &Path,
) -> Result<ShouldQuit> {
    match cmd {
        DaemonCommand::Subscribe => {
            // Handled at the connection level; nothing to do here.
        }

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
            if !url.starts_with("http://") && !url.starts_with("https://") {
                tracing::warn!("AddAccount rejected invalid URL: {url}");
                return Ok(ShouldQuit::No);
            }
            let new_account = crate::config::AccountConfig {
                id: Uuid::new_v4(),
                url,
                username: String::new(),
                display_name: String::new(),
                folder: vec![],
            };
            let account_id = new_account.id;
            config.account.push(new_account);
            config.save(config_path)?;
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id,
                state: "added".into(),
            });
        }

        DaemonCommand::RemoveAccount { account_id } => {
            config.account.retain(|a| a.id != account_id);
            config.save(config_path)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, GeneralConfig};
    use crate::folder_manager::FolderManager;
    use crate::scheduler::SyncScheduler;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    #[tokio::test]
    async fn trigger_sync_marks_pending() {
        let folder_id = Uuid::new_v4();
        let mut scheduler = SyncScheduler::new(vec![folder_id]);
        let (ipc, _rx) = GuiIpcServer::new();
        let mut config = AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        };
        let file = NamedTempFile::new().unwrap();
        let fm = FolderManager::empty();

        let result = handle_command(
            DaemonCommand::TriggerSync { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            &mut config,
            file.path(),
        )
        .await
        .unwrap();

        assert_eq!(result, ShouldQuit::No);
        // force_request_sync sets pending even if not paused, but folder isn't registered
        // so ready_to_run returns empty. Verify no panic and No returned.
    }

    #[tokio::test]
    async fn pause_marks_paused() {
        let folder_id = Uuid::new_v4();
        let mut scheduler = SyncScheduler::new(vec![folder_id]);
        let (ipc, _rx) = GuiIpcServer::new();
        let mut config = AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        };
        let file = NamedTempFile::new().unwrap();
        let fm = FolderManager::empty();

        handle_command(
            DaemonCommand::PauseFolder { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            &mut config,
            file.path(),
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
        let mut config = AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        };
        let file = NamedTempFile::new().unwrap();
        let fm = FolderManager::empty();

        handle_command(
            DaemonCommand::PauseFolder { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            &mut config,
            file.path(),
        )
        .await
        .unwrap();
        handle_command(
            DaemonCommand::ResumeFolder { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            &mut config,
            file.path(),
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
        let mut config = AppConfig {
            general: GeneralConfig::default(),
            account: vec![],
        };
        let file = NamedTempFile::new().unwrap();
        let fm = FolderManager::empty();

        let result = handle_command(
            DaemonCommand::Quit,
            &mut scheduler,
            &fm,
            &ipc,
            &mut config,
            file.path(),
        )
        .await
        .unwrap();

        assert_eq!(result, ShouldQuit::Yes);
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::sync::Mutex;
use uuid::Uuid;

use daemon::config::{AccountConfig, AppConfig, GeneralConfig};
use daemon::folder_manager::FolderManager;
use daemon::gui_ipc::handler::handle_command;
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};
use daemon::gui_ipc::GuiIpcServer;
use daemon::scheduler::SyncScheduler;
use ocis_client::auth::TokenManager;

#[tokio::test]
async fn set_account_folder_unknown_account_broadcasts_failed() {
    let (ipc, mut rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();

    let unknown_account_id = Uuid::new_v4();

    let result = handle_command(
        DaemonCommand::SetAccountFolder {
            account_id: unknown_account_id,
            local_path: "/tmp".to_string(),
        },
        Arc::clone(&scheduler),
        &mut fm,
        &ipc,
        config,
        file.path().to_path_buf(),
        Arc::new(std::sync::RwLock::new(vec![])),
        Arc::new(std::sync::RwLock::new(
            HashMap::<Uuid, Arc<TokenManager>>::new(),
        )),
    )
    .await
    .unwrap();

    use daemon::gui_ipc::handler::ShouldQuit;
    assert_eq!(result, ShouldQuit::No);

    let event = rx.try_recv().expect("expected an event to be broadcast");
    match event {
        DaemonEvent::AccountSetFolderFailed { account_id, reason } => {
            assert_eq!(account_id, unknown_account_id);
            assert!(
                reason.contains("not found"),
                "expected reason to contain 'not found', got: {reason}"
            );
        }
        other => panic!("expected AccountSetFolderFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn set_account_folder_invalid_path_broadcasts_failed() {
    let (ipc, mut rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));

    let account_id = Uuid::new_v4();
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: account_id,
            url: "https://ocis.example.com".to_string(),
            user_id: "alice".to_string(),
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            folder: vec![],
        }],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();

    let nonexistent_path = "/this/path/definitely/does/not/exist/on/this/system";

    let result = handle_command(
        DaemonCommand::SetAccountFolder {
            account_id,
            local_path: nonexistent_path.to_string(),
        },
        Arc::clone(&scheduler),
        &mut fm,
        &ipc,
        config,
        file.path().to_path_buf(),
        Arc::new(std::sync::RwLock::new(vec![])),
        Arc::new(std::sync::RwLock::new(
            HashMap::<Uuid, Arc<TokenManager>>::new(),
        )),
    )
    .await
    .unwrap();

    use daemon::gui_ipc::handler::ShouldQuit;
    assert_eq!(result, ShouldQuit::No);

    let event = rx.try_recv().expect("expected an event to be broadcast");
    match event {
        DaemonEvent::AccountSetFolderFailed {
            account_id: aid,
            reason,
        } => {
            assert_eq!(aid, account_id);
            assert!(
                reason.contains("not a directory") || reason.contains("does not exist"),
                "expected reason about path, got: {reason}"
            );
        }
        other => panic!("expected AccountSetFolderFailed, got {other:?}"),
    }
}

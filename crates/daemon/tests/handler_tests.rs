use std::collections::HashMap;
use std::sync::Arc;
use tempfile::{NamedTempFile, TempDir};
use tokio::sync::Mutex;
use uuid::Uuid;

use daemon::config::{AccountConfig, AppConfig, GeneralConfig};
use daemon::folder_manager::FolderManager;
use daemon::gui_ipc::handler::{handle_command, HandleContext, ShouldQuit};
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};
use daemon::gui_ipc::GuiIpcServer;
use daemon::scheduler::SyncScheduler;
use ocis_client::auth::TokenManager;

fn make_ctx<'a>(
    ipc: Arc<GuiIpcServer>,
    scheduler: Arc<Mutex<SyncScheduler>>,
    config: Arc<Mutex<AppConfig>>,
    file: &NamedTempFile,
    fm: &'a mut FolderManager,
    watcher_tx: tokio::sync::mpsc::Sender<(Uuid, notify::Event)>,
) -> HandleContext<'a> {
    HandleContext {
        scheduler,
        folder_manager: fm,
        ipc,
        config,
        config_path: file.path().to_path_buf(),
        live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
        token_managers: Arc::new(std::sync::RwLock::new(
            HashMap::<Uuid, Arc<TokenManager>>::new(),
        )),
        watcher_tx,
    }
}

#[tokio::test]
async fn list_spaces_unknown_account_broadcasts_failed() {
    let (ipc, mut rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();
    let (tx, _rx) = tokio::sync::mpsc::channel(1);

    let result = handle_command(
        DaemonCommand::ListSpaces {
            account_id: Uuid::new_v4(),
        },
        &mut make_ctx(ipc, scheduler, config, &file, &mut fm, tx),
    )
    .await
    .unwrap();

    assert_eq!(result, ShouldQuit::No);
    let event = rx.try_recv().expect("expected event");
    assert!(matches!(event, DaemonEvent::AccountSpaceFailed { .. }));
}

#[tokio::test]
async fn dismiss_space_adds_to_dismissed_list() {
    let (ipc, _rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let account_id = Uuid::new_v4();
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: account_id,
            url: "https://ocis.example.com".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            folder: vec![],
            dismissed_spaces: vec![],
        }],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();
    let (tx, _rx) = tokio::sync::mpsc::channel(1);

    handle_command(
        DaemonCommand::DismissSpace {
            account_id,
            space_id: "space-abc".into(),
        },
        &mut make_ctx(ipc, scheduler, config.clone(), &file, &mut fm, tx),
    )
    .await
    .unwrap();

    let cfg = config.lock().await;
    let acc = cfg.account.iter().find(|a| a.id == account_id).unwrap();
    assert!(acc.dismissed_spaces.contains(&"space-abc".to_string()));
}

#[tokio::test]
async fn set_account_folders_unknown_account_broadcasts_failed() {
    let (ipc, mut rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();
    let (tx, _rx) = tokio::sync::mpsc::channel(1);

    handle_command(
        DaemonCommand::SetAccountFolders {
            account_id: Uuid::new_v4(),
            root_path: "/tmp".into(),
            spaces: vec![],
        },
        &mut make_ctx(ipc, scheduler, config, &file, &mut fm, tx),
    )
    .await
    .unwrap();

    let event = rx.try_recv().expect("expected event");
    assert!(matches!(event, DaemonEvent::AccountSpaceFailed { .. }));
}

#[tokio::test]
async fn set_account_folders_no_token_manager_broadcasts_failed() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("ownCloud");
    std::fs::create_dir_all(&root).unwrap();

    let (ipc, mut rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let account_id = Uuid::new_v4();
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: account_id,
            url: "https://ocis.example.com".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            folder: vec![],
            dismissed_spaces: vec![],
        }],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();
    let (tx, _rx) = tokio::sync::mpsc::channel(256);

    handle_command(
        DaemonCommand::SetAccountFolders {
            account_id,
            root_path: root.to_string_lossy().into_owned(),
            spaces: vec![daemon::gui_ipc::protocol::SpaceSelection {
                space_id: "personal-space-id".into(),
                display_name: "Personal".into(),
            }],
        },
        &mut make_ctx(ipc, scheduler, config, &file, &mut fm, tx),
    )
    .await
    .unwrap();

    let event = rx.try_recv().expect("expected event");
    assert!(
        matches!(event, DaemonEvent::AccountSpaceFailed { .. }),
        "expected AccountSpaceFailed, got {event:?}"
    );
}

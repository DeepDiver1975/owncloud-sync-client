use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use uuid::Uuid;

use daemon::gui_ipc::protocol::{read_event, write_command, DaemonCommand, DaemonEvent};
use daemon::gui_ipc::GuiIpcServer;

async fn connect_client(socket_path: &std::path::Path) -> UnixStream {
    for _ in 0..20 {
        if let Ok(stream) = UnixStream::connect(socket_path).await {
            return stream;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("could not connect to GUI IPC socket");
}

#[tokio::test]
async fn subscribe_receives_ready_and_sync_started() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("daemon-gui.sock");

    let (ipc, _) = GuiIpcServer::new();
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<DaemonCommand>(64);

    let ipc_server = Arc::clone(&ipc);
    let sp = socket_path.clone();
    tokio::spawn(async move {
        let no_op: daemon::gui_ipc::SnapshotProvider = std::sync::Arc::new(|| {
            Box::pin(async { daemon::gui_ipc::protocol::DaemonEvent::Ready })
        });
        ipc_server.run(&sp, cmd_tx, no_op).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream_a = connect_client(&socket_path).await;
    let (mut read_a, mut write_a) = stream_a.into_split();
    write_command(&mut write_a, &DaemonCommand::Subscribe)
        .await
        .unwrap();

    let stream_b = connect_client(&socket_path).await;
    let (mut read_b, mut write_b) = stream_b.into_split();
    write_command(&mut write_b, &DaemonCommand::Subscribe)
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    // First event is the snapshot (no-op provider emits Ready)
    let _snap_a = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_a))
        .await
        .expect("timeout on client A snapshot")
        .unwrap();
    let _snap_b = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_b))
        .await
        .expect("timeout on client B snapshot")
        .unwrap();

    ipc.broadcast(DaemonEvent::Ready);

    let evt_a = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_a))
        .await
        .expect("timeout on client A")
        .unwrap();
    let evt_b = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_b))
        .await
        .expect("timeout on client B")
        .unwrap();

    assert_eq!(evt_a, DaemonEvent::Ready);
    assert_eq!(evt_b, DaemonEvent::Ready);

    let folder_id = Uuid::new_v4();

    let ipc_for_cmd = Arc::clone(&ipc);
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            if let DaemonCommand::TriggerSync { folder_id } = cmd {
                ipc_for_cmd.broadcast(DaemonEvent::SyncStarted { folder_id });
            }
        }
    });

    write_command(&mut write_a, &DaemonCommand::TriggerSync { folder_id })
        .await
        .unwrap();

    let evt_a2 = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_a))
        .await
        .expect("timeout on client A SyncStarted")
        .unwrap();
    let evt_b2 = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_b))
        .await
        .expect("timeout on client B SyncStarted")
        .unwrap();

    assert_eq!(evt_a2, DaemonEvent::SyncStarted { folder_id });
    assert_eq!(evt_b2, DaemonEvent::SyncStarted { folder_id });
}

#[tokio::test]
async fn non_subscribed_client_does_not_receive_events() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("daemon-gui-nosub.sock");

    let (ipc, _) = GuiIpcServer::new();
    let (cmd_tx, _cmd_rx) = mpsc::channel::<DaemonCommand>(64);

    let ipc_server = Arc::clone(&ipc);
    let sp = socket_path.clone();
    tokio::spawn(async move {
        let no_op: daemon::gui_ipc::SnapshotProvider = std::sync::Arc::new(|| {
            Box::pin(async { daemon::gui_ipc::protocol::DaemonEvent::Ready })
        });
        ipc_server.run(&sp, cmd_tx, no_op).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = connect_client(&socket_path).await;
    let (mut reader, _writer) = stream.into_split();

    ipc.broadcast(DaemonEvent::Ready);

    let result = tokio::time::timeout(Duration::from_millis(200), read_event(&mut reader)).await;

    assert!(result.is_err(), "expected timeout but got a message");
}

#[tokio::test]
async fn roundtrip_account_add_started() {
    use daemon::gui_ipc::protocol::{read_event, write_message, DaemonEvent};
    use tokio::io::duplex;
    use uuid::Uuid;

    let (mut client, mut server) = duplex(4096);
    let event = DaemonEvent::AccountAddStarted {
        account_id: Uuid::new_v4(),
    };
    write_message(&mut server, &event).await.unwrap();
    let received = read_event(&mut client).await.unwrap();
    assert_eq!(event, received);
}

#[tokio::test]
async fn add_account_invalid_url_broadcasts_failed() {
    use daemon::config::{AppConfig, GeneralConfig};
    use daemon::folder_manager::FolderManager;
    use daemon::gui_ipc::handler::handle_command;
    use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};
    use daemon::gui_ipc::GuiIpcServer;
    use daemon::scheduler::SyncScheduler;
    use std::sync::Arc;
    use tempfile::NamedTempFile;
    use tokio::sync::Mutex;

    let (ipc, mut event_rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();

    handle_command(
        DaemonCommand::AddAccount {
            url: "not-a-url".to_string(),
        },
        Arc::clone(&scheduler),
        &mut fm,
        &ipc,
        config,
        file.path().to_path_buf(),
        Arc::new(std::sync::RwLock::new(vec![])),
    )
    .await
    .unwrap();

    let evt = event_rx.try_recv().unwrap();
    assert!(
        matches!(evt, DaemonEvent::AccountAddFailed { .. }),
        "expected AccountAddFailed, got {evt:?}"
    );
}

#[tokio::test]
async fn roundtrip_account_add_failed() {
    use daemon::gui_ipc::protocol::{read_event, write_message, DaemonEvent};
    use tokio::io::duplex;
    use uuid::Uuid;

    let (mut client, mut server) = duplex(4096);
    let event = DaemonEvent::AccountAddFailed {
        account_id: Uuid::new_v4(),
        reason: "discovery failed".to_string(),
    };
    write_message(&mut server, &event).await.unwrap();
    let received = read_event(&mut client).await.unwrap();
    assert_eq!(event, received);
}

#[tokio::test]
async fn roundtrip_account_add_completed() {
    use daemon::gui_ipc::protocol::{read_event, write_message, DaemonEvent};
    use tokio::io::duplex;
    use uuid::Uuid;

    let (mut client, mut server) = duplex(4096);
    let event = DaemonEvent::AccountAddCompleted {
        account_id: Uuid::new_v4(),
        user_id: "uid-alice".to_string(),
        display_name: "Alice Hansen".to_string(),
        url: "https://cloud.example.com".to_string(),
    };
    write_message(&mut server, &event).await.unwrap();
    let received = read_event(&mut client).await.unwrap();
    assert_eq!(event, received);
}

#[tokio::test]
async fn roundtrip_account_set_folder_failed() {
    use daemon::gui_ipc::protocol::{read_event, write_message, DaemonEvent};
    use tokio::io::duplex;
    use uuid::Uuid;

    let (mut client, mut server) = duplex(4096);
    let event = DaemonEvent::AccountSetFolderFailed {
        account_id: Uuid::new_v4(),
        reason: "path does not exist".to_string(),
    };
    write_message(&mut server, &event).await.unwrap();
    let received = read_event(&mut client).await.unwrap();
    assert_eq!(event, received);
}

#[tokio::test]
async fn roundtrip_account_folder_added() {
    use daemon::gui_ipc::protocol::{read_event, write_message, DaemonEvent};
    use tokio::io::duplex;
    use uuid::Uuid;

    let (mut client, mut server) = duplex(4096);
    let event = DaemonEvent::AccountFolderAdded {
        account_id: Uuid::new_v4(),
        folder_id: Uuid::new_v4(),
        local_path: "/home/alice/ownCloud".to_string(),
        display_name: "Personal".to_string(),
    };
    write_message(&mut server, &event).await.unwrap();
    let received = read_event(&mut client).await.unwrap();
    assert_eq!(event, received);
}

#[tokio::test]
async fn roundtrip_set_account_folder_command() {
    use daemon::gui_ipc::protocol::{read_message, write_command, DaemonCommand};
    use tokio::io::duplex;
    use uuid::Uuid;

    let (mut client, mut server) = duplex(4096);
    let cmd = DaemonCommand::SetAccountFolder {
        account_id: Uuid::new_v4(),
        local_path: "/home/alice/ownCloud".to_string(),
    };
    write_command(&mut client, &cmd).await.unwrap();
    let received = read_message(&mut server).await.unwrap();
    assert_eq!(cmd, received);
}

#[tokio::test]
async fn subscribe_receives_account_snapshot_before_broadcasts() {
    use daemon::gui_ipc::protocol::{AccountSnapshot, DaemonEvent, FolderSnapshot};
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::net::UnixStream;
    use tokio::sync::mpsc;

    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("snapshot-test.sock");

    let account_id = uuid::Uuid::new_v4();
    let folder_id = uuid::Uuid::new_v4();

    let expected_snapshot = DaemonEvent::AccountSnapshot {
        accounts: vec![AccountSnapshot {
            account_id,
            url: "https://ocis.example.com".to_string(),
            display_name: "Alice".to_string(),
            folders: vec![FolderSnapshot {
                folder_id,
                display_name: "Personal".to_string(),
                local_path: "/home/alice/ownCloud".to_string(),
                status: "idle".to_string(),
            }],
        }],
    };

    let snapshot_clone = expected_snapshot.clone();
    let provider: daemon::gui_ipc::SnapshotProvider = Arc::new(move || {
        let evt = snapshot_clone.clone();
        Box::pin(async move { evt })
    });

    let (ipc, _) = daemon::gui_ipc::GuiIpcServer::new();
    let (cmd_tx, _cmd_rx) = mpsc::channel::<daemon::gui_ipc::protocol::DaemonCommand>(64);

    let ipc_server = Arc::clone(&ipc);
    let sp = socket_path.clone();
    tokio::spawn(async move {
        ipc_server.run(&sp, cmd_tx, provider).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&socket_path).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    daemon::gui_ipc::protocol::write_command(
        &mut writer,
        &daemon::gui_ipc::protocol::DaemonCommand::Subscribe,
    )
    .await
    .unwrap();

    let received = tokio::time::timeout(
        Duration::from_secs(1),
        daemon::gui_ipc::protocol::read_event(&mut reader),
    )
    .await
    .expect("timeout waiting for AccountSnapshot")
    .unwrap();

    assert_eq!(received, expected_snapshot);
}

#[tokio::test]
async fn roundtrip_account_snapshot() {
    use daemon::gui_ipc::protocol::{
        read_event, write_message, AccountSnapshot, DaemonEvent, FolderSnapshot,
    };
    use tokio::io::duplex;
    use uuid::Uuid;

    let (mut client, mut server) = duplex(4096);
    let event = DaemonEvent::AccountSnapshot {
        accounts: vec![AccountSnapshot {
            account_id: Uuid::new_v4(),
            url: "https://ocis.example.com".to_string(),
            display_name: "Alice".to_string(),
            folders: vec![FolderSnapshot {
                folder_id: Uuid::new_v4(),
                display_name: "Personal".to_string(),
                local_path: "/home/alice/ownCloud".to_string(),
                status: "idle".to_string(),
            }],
        }],
    };
    write_message(&mut server, &event).await.unwrap();
    let received = read_event(&mut client).await.unwrap();
    assert_eq!(event, received);
}

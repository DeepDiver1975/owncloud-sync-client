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
        ipc_server.run(&sp, cmd_tx).await.unwrap();
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
        ipc_server.run(&sp, cmd_tx).await.unwrap();
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

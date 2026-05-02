use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};
use gui::daemon_conn::DaemonConnection;
use uuid::Uuid;

#[cfg(unix)]
fn unique_socket() -> std::path::PathBuf {
    std::path::PathBuf::from(format!("/tmp/ocsync_conn_test_{}.sock", Uuid::new_v4()))
}

#[test]
fn disconnected_send_does_not_panic() {
    let conn = DaemonConnection::disconnected();
    conn.send(DaemonCommand::Quit);
}

#[cfg(unix)]
#[tokio::test]
async fn connect_returns_connection_and_event_receiver() {
    use tokio::net::UnixListener;

    let path = unique_socket();
    let listener = UnixListener::bind(&path).expect("bind");

    let path2 = path.clone();
    let result = DaemonConnection::connect(&path2).await;
    drop(listener);
    let _ = std::fs::remove_file(&path);

    assert!(result.is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn events_received_from_daemon() {
    use daemon::gui_ipc::protocol::write_message;
    use tokio::net::UnixListener;

    let path = unique_socket();
    let listener = UnixListener::bind(&path).expect("bind");

    let path2 = path.clone();
    let (conn, mut event_rx) = DaemonConnection::connect(&path2).await.expect("connect");
    drop(conn);

    let (stream, _) = listener.accept().await.expect("accept");
    let (_, mut write_half) = stream.into_split();

    let event = DaemonEvent::Ready;
    write_message(&mut write_half, &event).await.expect("write");

    let received = tokio::time::timeout(std::time::Duration::from_secs(1), event_rx.recv())
        .await
        .expect("timeout")
        .expect("event");

    assert!(matches!(received, DaemonEvent::Ready));

    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[tokio::test]
async fn commands_sent_to_daemon() {
    use daemon::gui_ipc::protocol::read_message;
    use tokio::net::UnixListener;

    let path = unique_socket();
    let listener = UnixListener::bind(&path).expect("bind");

    let path2 = path.clone();
    let (conn, _event_rx) = DaemonConnection::connect(&path2).await.expect("connect");

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut read_half, _) = stream.into_split();

    // connect() sends Subscribe first; consume it before our command arrives.
    let subscribe = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        read_message(&mut read_half),
    )
    .await
    .expect("timeout")
    .expect("read");
    assert!(matches!(subscribe, DaemonCommand::Subscribe));

    conn.send(DaemonCommand::TriggerSync {
        folder_id: Uuid::nil(),
    });

    let parsed = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        read_message(&mut read_half),
    )
    .await
    .expect("timeout")
    .expect("read");

    assert!(matches!(parsed, DaemonCommand::TriggerSync { .. }));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn send_returns_false_when_disconnected() {
    let conn = DaemonConnection::disconnected();
    let sent = conn.send(DaemonCommand::Quit);
    assert!(!sent, "expected false when channel receiver is dropped");
}

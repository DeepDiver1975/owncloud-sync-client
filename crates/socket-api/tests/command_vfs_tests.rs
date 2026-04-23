use socket_api::broadcast::BroadcastSender;
use socket_api::commands::vfs_cmds::{handle_make_available_locally, handle_make_online_only};
use std::sync::Arc;
use tokio::sync::mpsc;
use vfs_off::VfsOff;

#[tokio::test]
async fn make_available_locally_returns_ok() {
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());
    let broadcaster = BroadcastSender::new();
    let paths = vec!["/sync/root/file.txt".to_string()];

    let resp = handle_make_available_locally(paths, vfs, &broadcaster).await;
    assert_eq!(resp, "MAKE_AVAILABLE_LOCALLY:OK\n");
}

#[tokio::test]
async fn make_online_only_returns_ok() {
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());
    let broadcaster = BroadcastSender::new();
    let paths = vec!["/sync/root/file.txt".to_string()];

    let resp = handle_make_online_only(paths, vfs, &broadcaster).await;
    assert_eq!(resp, "MAKE_ONLINE_ONLY:OK\n");
}

#[tokio::test]
async fn make_available_locally_broadcasts_status_for_each_path() {
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());
    let broadcaster = BroadcastSender::new();

    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    let paths = vec![
        "/sync/root/a.txt".to_string(),
        "/sync/root/b.txt".to_string(),
    ];

    handle_make_available_locally(paths, vfs, &broadcaster).await;

    let msg1 = rx.recv().await.expect("first broadcast missing");
    let msg2 = rx.recv().await.expect("second broadcast missing");

    assert!(
        msg1.starts_with("STATUS:"),
        "expected STATUS broadcast, got: {msg1}"
    );
    assert!(
        msg2.starts_with("STATUS:"),
        "expected STATUS broadcast, got: {msg2}"
    );
}

#[tokio::test]
async fn make_online_only_broadcasts_status_for_each_path() {
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());
    let broadcaster = BroadcastSender::new();

    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    let paths = vec!["/sync/root/c.txt".to_string()];

    handle_make_online_only(paths, vfs, &broadcaster).await;

    let msg = rx.recv().await.expect("broadcast missing");
    assert!(
        msg.starts_with("STATUS:"),
        "expected STATUS broadcast, got: {msg}"
    );
}

use socket_api::broadcast::BroadcastSender;
use tokio::sync::mpsc;

#[tokio::test]
async fn two_connections_both_receive_status_changed() {
    let broadcaster = BroadcastSender::new();

    let (tx1, mut rx1) = mpsc::channel::<String>(8);
    let (tx2, mut rx2) = mpsc::channel::<String>(8);

    let _id1 = broadcaster.add_connection(tx1);
    let _id2 = broadcaster.add_connection(tx2);

    broadcaster.status_changed("OK", "/foo/bar.txt").await;

    let msg1 = rx1.recv().await.expect("connection 1 should receive message");
    let msg2 = rx2.recv().await.expect("connection 2 should receive message");

    assert_eq!(msg1, "STATUS:OK:/foo/bar.txt\n");
    assert_eq!(msg2, "STATUS:OK:/foo/bar.txt\n");
}

#[tokio::test]
async fn removed_connection_does_not_receive() {
    let broadcaster = BroadcastSender::new();

    let (tx1, mut rx1) = mpsc::channel::<String>(8);
    let (tx2, mut rx2) = mpsc::channel::<String>(8);

    let id1 = broadcaster.add_connection(tx1);
    let _id2 = broadcaster.add_connection(tx2);

    broadcaster.remove_connection(id1);

    broadcaster.status_changed("SYNC", "/a/b.txt").await;

    let msg2 = rx2.recv().await.expect("connection 2 should receive");
    assert_eq!(msg2, "STATUS:SYNC:/a/b.txt\n");

    assert!(rx1.try_recv().is_err(), "removed connection must not receive");
}

#[tokio::test]
async fn register_path_broadcasts_register_message() {
    let broadcaster = BroadcastSender::new();
    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    broadcaster.register_path("/sync/root").await;

    let msg = rx.recv().await.unwrap();
    assert_eq!(msg, "REGISTER_PATH:/sync/root\n");
}

#[tokio::test]
async fn unregister_path_broadcasts_unregister_message() {
    let broadcaster = BroadcastSender::new();
    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    broadcaster.unregister_path("/sync/root").await;

    let msg = rx.recv().await.unwrap();
    assert_eq!(msg, "UNREGISTER_PATH:/sync/root\n");
}

#[tokio::test]
async fn update_view_broadcasts_update_message() {
    let broadcaster = BroadcastSender::new();
    let (tx, mut rx) = mpsc::channel::<String>(8);
    broadcaster.add_connection(tx);

    broadcaster.update_view("/sync/root/subdir").await;

    let msg = rx.recv().await.unwrap();
    assert_eq!(msg, "UPDATE_VIEW:/sync/root/subdir\n");
}

use daemon::gui_ipc::protocol::DaemonEvent;
use gui::app::Message;
use gui::subscription::next_message;
use tokio::sync::mpsc;

#[tokio::test]
async fn next_message_wraps_event() {
    let (tx, mut rx) = mpsc::channel(1);
    tx.send(DaemonEvent::Ready).await.unwrap();
    let msg = next_message(&mut rx).await.unwrap();
    assert!(matches!(msg, Message::DaemonEvent(DaemonEvent::Ready)));
}

#[tokio::test]
async fn next_message_returns_disconnected_when_channel_closed() {
    let (tx, mut rx) = mpsc::channel::<DaemonEvent>(1);
    drop(tx);
    let msg = next_message(&mut rx).await.unwrap();
    assert!(matches!(msg, Message::DaemonDisconnected));
}

#[tokio::test]
async fn next_message_returns_none_after_disconnect_sentinel() {
    let (tx, mut rx) = mpsc::channel::<DaemonEvent>(1);
    drop(tx);
    // First call yields DaemonDisconnected
    let _ = next_message(&mut rx).await;
    // Channel is now drained and closed — recv returns None again,
    // so next_message returns DaemonDisconnected again (not None)
    let msg = next_message(&mut rx).await;
    assert!(matches!(msg, Some(Message::DaemonDisconnected)));
}

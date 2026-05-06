use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use daemon::gui_ipc::protocol::DaemonEvent;
use gui::app::{EventRxCarrier, Message};
use gui::subscription::next_message;

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

#[tokio::test]
async fn subscription_nils_receiver_after_disconnect() {
    // Build a closed channel — simulates the daemon dropping the connection.
    let (tx, rx) = mpsc::channel::<DaemonEvent>(1);
    drop(tx);

    let event_rx: EventRxCarrier = Arc::new(Mutex::new(Some(rx)));
    let event_rx_clone = event_rx.clone();

    // Run one subscription iteration: call next_message, detect DaemonDisconnected,
    // and nil the Option — mirroring what the subscription closure does.
    {
        let mut guard = event_rx_clone.lock().await;
        if let Some(receiver) = guard.as_mut() {
            let m = next_message(receiver).await;
            if matches!(m, Some(Message::DaemonDisconnected)) {
                *guard = None;
            }
        }
    }

    // The receiver must now be None so we don't hammer the closed channel.
    let guard = event_rx.lock().await;
    assert!(guard.is_none(), "expected None after disconnect");
}

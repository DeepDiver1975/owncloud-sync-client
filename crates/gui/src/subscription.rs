use tokio::sync::mpsc;

use daemon::gui_ipc::protocol::DaemonEvent;

use crate::app::Message;

/// Pull the next item from the receiver and convert it to a `Message`.
/// Returns `None` after `DaemonDisconnected` has been emitted (channel closed).
pub async fn next_message(rx: &mut mpsc::Receiver<DaemonEvent>) -> Option<Message> {
    match rx.recv().await {
        Some(event) => Some(Message::DaemonEvent(event)),
        None => Some(Message::DaemonDisconnected),
    }
}

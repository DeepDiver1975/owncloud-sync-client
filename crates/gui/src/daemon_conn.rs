use std::path::Path;

use thiserror::Error;
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use daemon::gui_ipc::protocol::{read_event, write_command, DaemonCommand, DaemonEvent};

#[derive(Debug, Error)]
pub enum ConnError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct DaemonConnection {
    cmd_tx: mpsc::Sender<DaemonCommand>,
}

impl DaemonConnection {
    pub fn disconnected() -> Self {
        let (tx, _rx) = mpsc::channel(1);
        Self { cmd_tx: tx }
    }

    pub async fn connect(
        socket_path: &Path,
    ) -> Result<(Self, mpsc::Receiver<DaemonEvent>), ConnError> {
        let stream = UnixStream::connect(socket_path).await?;
        let (mut read_half, mut write_half) = stream.into_split();

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<DaemonCommand>(64);
        let (event_tx, event_rx) = mpsc::channel::<DaemonEvent>(64);

        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                if write_command(&mut write_half, &cmd).await.is_err() {
                    tracing::warn!("daemon write half closed");
                    break;
                }
            }
        });

        tokio::spawn(async move {
            loop {
                match read_event(&mut read_half).await {
                    Ok(event) => {
                        if event_tx.send(event).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let is_disconnect = e.downcast_ref::<std::io::Error>()
                            .map(|io_err| matches!(
                                io_err.kind(),
                                std::io::ErrorKind::UnexpectedEof
                                    | std::io::ErrorKind::ConnectionReset
                            ))
                            .unwrap_or(false);
                        if is_disconnect {
                            tracing::info!("daemon closed connection");
                        } else {
                            tracing::warn!("daemon read error: {e}");
                        }
                        break;
                    }
                }
            }
        });

        Ok((Self { cmd_tx }, event_rx))
    }

    pub fn send(&self, cmd: DaemonCommand) {
        if let Err(e) = self.cmd_tx.try_send(cmd) {
            tracing::warn!("failed to send daemon command: {e}");
        }
    }
}

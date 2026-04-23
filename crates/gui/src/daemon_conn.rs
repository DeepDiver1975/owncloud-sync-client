use std::path::Path;

use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};

#[derive(Debug, Error)]
pub enum ConnError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
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

    pub fn from_sender(cmd_tx: mpsc::Sender<DaemonCommand>) -> Self {
        Self { cmd_tx }
    }

    pub async fn connect(
        socket_path: &Path,
    ) -> Result<(Self, mpsc::Receiver<DaemonEvent>), ConnError> {
        let stream = UnixStream::connect(socket_path).await?;
        let (read_half, mut write_half) = stream.into_split();

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<DaemonCommand>(64);
        let (event_tx, event_rx) = mpsc::channel::<DaemonEvent>(64);

        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                match serde_json::to_string(&cmd) {
                    Ok(json) => {
                        let line = json + "\n";
                        if write_half.write_all(line.as_bytes()).await.is_err() {
                            tracing::warn!("daemon write half closed");
                            break;
                        }
                    }
                    Err(e) => tracing::error!("failed to serialize command: {e}"),
                }
            }
        });

        tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        tracing::info!("daemon closed connection (EOF)");
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() { continue; }
                        match serde_json::from_str::<DaemonEvent>(trimmed) {
                            Ok(event) => {
                                if event_tx.send(event).await.is_err() { break; }
                            }
                            Err(e) => {
                                tracing::warn!("failed to parse daemon event: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("daemon read error: {e}");
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

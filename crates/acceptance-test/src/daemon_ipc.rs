use anyhow::Result;
use std::path::Path;
use std::time::Duration;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use daemon::gui_ipc::protocol::{read_event, write_command, DaemonCommand, DaemonEvent};

pub struct DaemonIpcClient {
    writer: tokio::net::unix::OwnedWriteHalf,
    event_rx: mpsc::Receiver<DaemonEvent>,
}

impl DaemonIpcClient {
    pub async fn connect(socket_path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket_path).await?;
        let (reader, writer) = stream.into_split();
        let mut buf_reader = BufReader::new(reader);

        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(async move {
            loop {
                match read_event(&mut buf_reader).await {
                    Ok(event) => {
                        if tx.send(event).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut client = Self {
            writer,
            event_rx: rx,
        };

        client.send(DaemonCommand::Subscribe).await?;
        Ok(client)
    }

    pub async fn send(&mut self, cmd: DaemonCommand) -> Result<()> {
        write_command(&mut self.writer, &cmd).await
    }

    pub async fn next_event(&mut self, timeout: Duration) -> Option<DaemonEvent> {
        tokio::time::timeout(timeout, self.event_rx.recv())
            .await
            .ok()
            .flatten()
    }

    pub async fn wait_for<F>(&mut self, predicate: F, timeout: Duration) -> Option<DaemonEvent>
    where
        F: Fn(&DaemonEvent) -> bool,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match tokio::time::timeout(remaining, self.event_rx.recv()).await {
                Ok(Some(event)) if predicate(&event) => return Some(event),
                Ok(Some(_)) => continue,
                _ => return None,
            }
        }
    }
}

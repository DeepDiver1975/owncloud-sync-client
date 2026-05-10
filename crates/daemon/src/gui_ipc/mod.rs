pub mod handler;
pub mod protocol;

use anyhow::Result;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

use protocol::{read_message, write_message, DaemonCommand, DaemonEvent};

const BROADCAST_CAPACITY: usize = 256;

pub type SnapshotProvider =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = DaemonEvent> + Send>> + Send + Sync>;

pub struct GuiIpcServer {
    pub event_tx: broadcast::Sender<DaemonEvent>,
}

impl GuiIpcServer {
    pub fn new() -> (Arc<Self>, broadcast::Receiver<DaemonEvent>) {
        let (tx, rx) = broadcast::channel(BROADCAST_CAPACITY);
        (Arc::new(Self { event_tx: tx }), rx)
    }

    pub fn broadcast(&self, event: DaemonEvent) {
        if self.event_tx.send(event).is_err() {
            tracing::warn!("broadcast: no GUI clients connected, event dropped");
        }
    }

    pub async fn run(
        self: Arc<Self>,
        socket_path: &Path,
        cmd_tx: mpsc::Sender<DaemonCommand>,
        snapshot: SnapshotProvider,
    ) -> Result<()> {
        let _ = std::fs::remove_file(socket_path);
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(socket_path)?;
        info!("GUI IPC listening on {}", socket_path.display());

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let server = Arc::clone(&self);
                    let tx = cmd_tx.clone();
                    let snap = Arc::clone(&snapshot);
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(server, stream, tx, snap).await {
                            debug!("GUI IPC connection closed: {e}");
                        }
                    });
                }
                Err(e) => {
                    error!("GUI IPC accept error: {e}");
                    break;
                }
            }
        }
        Ok(())
    }
}

async fn handle_connection(
    server: Arc<GuiIpcServer>,
    stream: UnixStream,
    cmd_tx: mpsc::Sender<DaemonCommand>,
    snapshot: SnapshotProvider,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader);

    let mut event_rx: Option<broadcast::Receiver<DaemonEvent>> = None;

    loop {
        if let Some(rx) = event_rx.as_mut() {
            tokio::select! {
                cmd_result = read_message(&mut reader) => {
                    match cmd_result {
                        Ok(cmd) => { cmd_tx.send(cmd).await?; }
                        Err(_) => break,
                    }
                }
                evt_result = rx.recv() => {
                    match evt_result {
                        Ok(evt) => { write_message(&mut writer, &evt).await?; }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("GUI IPC client lagged, dropped {n} events");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        } else {
            match read_message(&mut reader).await {
                Ok(DaemonCommand::Subscribe) => {
                    // Subscribe to broadcast first so we don't miss any events
                    // that arrive between snapshot generation and loop entry.
                    let rx = server.event_tx.subscribe();
                    let snap_event = snapshot().await;
                    write_message(&mut writer, &snap_event).await?;
                    event_rx = Some(rx);
                }
                Ok(cmd) => {
                    cmd_tx.send(cmd).await?;
                }
                Err(_) => break,
            }
        }
    }
    Ok(())
}

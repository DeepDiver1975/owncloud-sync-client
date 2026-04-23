pub mod handler;
pub mod protocol;

use std::path::Path;
use std::sync::Arc;
use anyhow::Result;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

use protocol::{DaemonCommand, DaemonEvent, read_message, write_message};

const BROADCAST_CAPACITY: usize = 256;

pub struct GuiIpcServer {
    pub event_tx: broadcast::Sender<DaemonEvent>,
}

impl GuiIpcServer {
    pub fn new() -> (Arc<Self>, broadcast::Receiver<DaemonEvent>) {
        let (tx, rx) = broadcast::channel(BROADCAST_CAPACITY);
        (Arc::new(Self { event_tx: tx }), rx)
    }

    pub fn broadcast(&self, event: DaemonEvent) {
        let _ = self.event_tx.send(event);
    }

    pub async fn run(
        self: Arc<Self>,
        socket_path: &Path,
        cmd_tx: mpsc::Sender<DaemonCommand>,
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
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(server, stream, tx).await {
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
                    event_rx = Some(server.event_tx.subscribe());
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

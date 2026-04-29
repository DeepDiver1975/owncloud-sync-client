use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use camino::Utf8PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::broadcast::BroadcastSender;
use crate::commands::menu::{handle_get_menu_items, handle_get_strings};
use crate::commands::share::{handle_copy_private_link, handle_share};
use crate::commands::status::{handle_retrieve_file_status, handle_retrieve_folder_status};
use crate::commands::v2::handle_v2_get_client_icon;
use crate::commands::vfs_cmds::{handle_make_available_locally, handle_make_online_only};
use crate::error::{Result, SocketApiError};
use crate::protocol::{parse_command, Command};
use crate::status_resolver::StatusResolver;
use crate::transport::{Connection, Transport};
use sync_engine::state::SyncState;
use vfs_core::Vfs;

pub struct SocketApiServer {
    resolver: Arc<StatusResolver>,
    broadcast: Arc<BroadcastSender>,
    vfs: Arc<dyn Vfs>,
    folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>>,
}

impl SocketApiServer {
    pub fn new(
        sync_states: Arc<RwLock<HashMap<Uuid, SyncState>>>,
        folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>>,
        vfs: Arc<dyn Vfs>,
    ) -> Self {
        let resolver = Arc::new(StatusResolver::new(sync_states, folder_roots.clone()));
        let broadcast = Arc::new(BroadcastSender::new());
        Self {
            resolver,
            broadcast,
            vfs,
            folder_roots,
        }
    }

    pub fn broadcast(&self) -> Arc<BroadcastSender> {
        self.broadcast.clone()
    }

    pub async fn run(self: Arc<Self>, transport: Box<dyn Transport>) -> Result<()> {
        loop {
            let conn = transport
                .accept()
                .await
                .map_err(|e| SocketApiError::Transport(format!("accept error: {e}")))?;

            let server = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server.handle_connection(conn).await {
                    tracing::warn!("connection error: {e}");
                }
            });
        }
    }

    async fn handle_connection(self: Arc<Self>, conn: Connection) -> Result<()> {
        let (tx, mut broadcast_rx) = mpsc::channel::<String>(64);
        let conn_id = self.broadcast.add_connection(tx);

        // Split into independent read and write halves so the broadcast writer
        // doesn't block on the read loop and vice versa.
        let (read_half, write_half) = tokio::io::split(conn);
        let write_half = Arc::new(tokio::sync::Mutex::new(write_half));

        {
            let register_msgs: Vec<String> = {
                let roots = self.folder_roots.read().unwrap();
                roots
                    .iter()
                    .map(|(root, _)| format!("REGISTER_PATH:{root}\n"))
                    .collect()
            };
            let mut guard = write_half.lock().await;
            for msg in register_msgs {
                if guard.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
            }
        }

        let write_half_broadcast = write_half.clone();
        let write_task = tokio::spawn(async move {
            while let Some(msg) = broadcast_rx.recv().await {
                let mut guard = write_half_broadcast.lock().await;
                if guard.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        loop {
            line.clear();
            let n = match reader.read_line(&mut line).await {
                Ok(n) => n,
                Err(e) => {
                    tracing::debug!("read error: {e}");
                    0
                }
            };

            if n == 0 {
                break;
            }

            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
            if trimmed.is_empty() {
                continue;
            }

            if let Some(response) = self.dispatch_line(trimmed).await {
                let mut guard = write_half.lock().await;
                if guard.write_all(response.as_bytes()).await.is_err() {
                    break;
                }
            }
        }

        self.broadcast.remove_connection(conn_id);
        write_task.abort();

        Ok(())
    }

    pub async fn dispatch_line(&self, line: &str) -> Option<String> {
        let cmd = match parse_command(line) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("parse error for {:?}: {e}", line);
                return None;
            }
        };

        let response = match cmd {
            Command::Version => "VERSION:1.1\n".to_string(),
            Command::GetStrings => handle_get_strings(),
            Command::GetMenuItems { path } => handle_get_menu_items(&path, &self.resolver),
            Command::RetrieveFileStatus { path } => {
                handle_retrieve_file_status(&path, &self.resolver)
            }
            Command::RetrieveFolderStatus { path } => {
                handle_retrieve_folder_status(&path, &self.resolver)
            }
            Command::Share { path } => handle_share(&path),
            Command::CopyPrivateLink { path } => handle_copy_private_link(&path),
            Command::MakeAvailableLocally { paths } => {
                handle_make_available_locally(paths, self.vfs.clone(), &self.broadcast).await
            }
            Command::MakeOnlineOnly { paths } => {
                handle_make_online_only(paths, self.vfs.clone(), &self.broadcast).await
            }
            Command::V2 { name, body } if name == "GET_CLIENT_ICON" => {
                handle_v2_get_client_icon(&body)
            }
            Command::V2 { name, .. } => {
                tracing::debug!("unhandled V2 command: {name}");
                return None;
            }
        };

        Some(response)
    }
}

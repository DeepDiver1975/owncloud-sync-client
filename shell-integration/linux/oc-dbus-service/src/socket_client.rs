use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Debug, Error)]
pub enum SocketError {
    #[error("connect error: {0}")]
    Connect(#[source] std::io::Error),
    #[error("I/O error: {0}")]
    Io(#[source] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
}

pub struct SocketClient {
    writer: tokio::net::unix::OwnedWriteHalf,
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
}

pub enum Broadcast {
    Status { tag: String, path: String },
    RegisterPath(String),
    UpdateView(String),
    Unknown(String),
}

impl SocketClient {
    pub async fn connect_path(path: &str) -> Result<Self, SocketError> {
        let stream = UnixStream::connect(path)
            .await
            .map_err(SocketError::Connect)?;
        let (read_half, write_half) = stream.into_split();
        Ok(Self {
            writer: write_half,
            reader: BufReader::new(read_half),
        })
    }

    pub async fn connect() -> Result<Self, SocketError> {
        let xdg_runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        let primary = format!("{xdg_runtime}/owncloud/socket");
        let fallback = format!(
            "{}/.local/share/owncloud/socket",
            std::env::var("HOME").unwrap_or_default()
        );
        if std::path::Path::new(&primary).exists() {
            Self::connect_path(&primary).await
        } else {
            Self::connect_path(&fallback).await
        }
    }

    pub async fn get_file_status(&mut self, path: &str) -> Result<String, SocketError> {
        let cmd = format!("RETRIEVE_FILE_STATUS:{path}\n");
        self.writer
            .write_all(cmd.as_bytes())
            .await
            .map_err(SocketError::Io)?;
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .await
            .map_err(SocketError::Io)?;
        parse_status_line(&line)
            .map(|(tag, _)| tag)
            .ok_or_else(|| SocketError::Parse(format!("unexpected status response: {line}")))
    }

    pub async fn get_menu_items(
        &mut self,
        path: &str,
    ) -> Result<Vec<(String, String, bool)>, SocketError> {
        let cmd = format!("GET_MENU_ITEMS:{path}\n");
        self.writer
            .write_all(cmd.as_bytes())
            .await
            .map_err(SocketError::Io)?;
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .await
            .map_err(SocketError::Io)?;
        Ok(parse_menu_items_line(&line))
    }

    pub async fn execute_command(
        &mut self,
        command: &str,
        paths: &[String],
    ) -> Result<(), SocketError> {
        let joined = paths.join("\x1e");
        let cmd = format!("{command}:{joined}\n");
        self.writer
            .write_all(cmd.as_bytes())
            .await
            .map_err(SocketError::Io)?;
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .await
            .map_err(SocketError::Io)?;
        Ok(())
    }

    pub async fn read_broadcast(&mut self) -> Result<Broadcast, SocketError> {
        let mut line = String::new();
        let n = self
            .reader
            .read_line(&mut line)
            .await
            .map_err(SocketError::Io)?;
        if n == 0 {
            return Err(SocketError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed",
            )));
        }
        let trimmed = line.trim();
        if let Some((tag, path)) = parse_status_line(trimmed) {
            return Ok(Broadcast::Status { tag, path });
        }
        if let Some(rest) = trimmed.strip_prefix("REGISTER_PATH:") {
            return Ok(Broadcast::RegisterPath(rest.to_string()));
        }
        if let Some(rest) = trimmed.strip_prefix("UPDATE_VIEW:") {
            return Ok(Broadcast::UpdateView(rest.to_string()));
        }
        Ok(Broadcast::Unknown(trimmed.to_string()))
    }
}

pub fn parse_status_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("STATUS:")?;
    // FORMAT: STATUS:TAG:path — path may contain colons
    let colon = rest.find(':')?;
    let tag = rest[..colon].to_string();
    let path = rest[colon + 1..].to_string();
    Some((tag, path))
}

pub fn parse_menu_items_line(line: &str) -> Vec<(String, String, bool)> {
    let trimmed = line.trim();
    let parts: Vec<&str> = trimmed.split('\x1e').collect();
    // First part is "GET_MENU_ITEMS:/path", remaining are "name:cmd:state"
    parts
        .iter()
        .skip(1)
        .filter_map(|part| {
            let fields: Vec<&str> = part.splitn(3, ':').collect();
            if fields.len() == 3 {
                let name = fields[0].to_string();
                let cmd = fields[1].to_string();
                let enabled = fields[2] == "enabled";
                Some((name, cmd, enabled))
            } else {
                None
            }
        })
        .collect()
}

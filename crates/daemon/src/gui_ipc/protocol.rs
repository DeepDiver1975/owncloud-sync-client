use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;
use anyhow::{bail, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum DaemonCommand {
    Subscribe,
    TriggerSync { folder_id: Uuid },
    PauseFolder  { folder_id: Uuid },
    ResumeFolder { folder_id: Uuid },
    AddAccount   { url: String },
    RemoveAccount { account_id: Uuid },
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum DaemonEvent {
    Ready,
    SyncStarted   { folder_id: Uuid },
    SyncProgress  { folder_id: Uuid, done: u64, total: u64 },
    SyncFinished  { folder_id: Uuid, errors: Vec<String> },
    FileStatusChanged  { path: String, status: String },
    AccountStateChanged { account_id: Uuid, state: String },
}

pub async fn write_message<W: AsyncWrite + Unpin>(w: &mut W, event: &DaemonEvent) -> Result<()> {
    let json = serde_json::to_vec(event)?;
    let len = json.len() as u32;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&json).await?;
    Ok(())
}

pub async fn read_message<R: AsyncRead + Unpin>(r: &mut R) -> Result<DaemonCommand> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 4 * 1024 * 1024 {
        bail!("incoming message too large: {} bytes", len);
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    let cmd: DaemonCommand = serde_json::from_slice(&buf)?;
    Ok(cmd)
}

pub async fn write_command<W: AsyncWrite + Unpin>(w: &mut W, cmd: &DaemonCommand) -> Result<()> {
    let json = serde_json::to_vec(cmd)?;
    let len = json.len() as u32;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&json).await?;
    Ok(())
}

pub async fn read_event<R: AsyncRead + Unpin>(r: &mut R) -> Result<DaemonEvent> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    let evt: DaemonEvent = serde_json::from_slice(&buf)?;
    Ok(evt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn roundtrip(cmd: DaemonCommand) {
        let (mut client, mut server) = duplex(4096);
        write_command(&mut client, &cmd).await.unwrap();
        let received = read_message(&mut server).await.unwrap();
        assert_eq!(cmd, received);
    }

    #[tokio::test]
    async fn roundtrip_subscribe() { roundtrip(DaemonCommand::Subscribe).await; }

    #[tokio::test]
    async fn roundtrip_trigger_sync() {
        roundtrip(DaemonCommand::TriggerSync { folder_id: Uuid::new_v4() }).await;
    }

    #[tokio::test]
    async fn roundtrip_pause_folder() {
        roundtrip(DaemonCommand::PauseFolder { folder_id: Uuid::new_v4() }).await;
    }

    #[tokio::test]
    async fn roundtrip_resume_folder() {
        roundtrip(DaemonCommand::ResumeFolder { folder_id: Uuid::new_v4() }).await;
    }

    #[tokio::test]
    async fn roundtrip_add_account() {
        roundtrip(DaemonCommand::AddAccount { url: "https://ocis.example.com".into() }).await;
    }

    #[tokio::test]
    async fn roundtrip_remove_account() {
        roundtrip(DaemonCommand::RemoveAccount { account_id: Uuid::new_v4() }).await;
    }

    #[tokio::test]
    async fn roundtrip_quit() { roundtrip(DaemonCommand::Quit).await; }

    #[tokio::test]
    async fn event_write_read_roundtrip() {
        let (mut client, mut server) = duplex(4096);
        let event = DaemonEvent::SyncProgress {
            folder_id: Uuid::new_v4(),
            done: 42,
            total: 100,
        };
        write_message(&mut server, &event).await.unwrap();
        let received = read_event(&mut client).await.unwrap();
        assert_eq!(event, received);
    }
}

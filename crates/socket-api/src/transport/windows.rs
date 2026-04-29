#![cfg(target_os = "windows")]

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

use crate::error::{Result, SocketApiError};
use crate::transport::{Connection, Transport};

pub struct WindowsTransport {
    pipe_name: String,
}

impl WindowsTransport {
    pub fn bind() -> Result<Self> {
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "ownCloud".into());
        let pipe_name = format!(r"\\.\pipe\ownCloud-{username}");
        Ok(Self { pipe_name })
    }

    async fn accept_inner(&self) -> Result<NamedPipeServer> {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(&self.pipe_name)
            .map_err(|e| SocketApiError::Transport(format!("named pipe create failed: {e}")))?;

        server
            .connect()
            .await
            .map_err(|e| SocketApiError::Transport(format!("named pipe connect failed: {e}")))?;

        Ok(server)
    }
}

#[async_trait::async_trait]
impl Transport for WindowsTransport {
    async fn accept(&self) -> Result<Connection> {
        let pipe = self.accept_inner().await?;
        Ok(Box::new(pipe))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires Windows"]
    fn windows_transport_binds() {}
}

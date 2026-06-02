use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::Result;

pub trait AsyncReadWrite: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncReadWrite for T {}

pub type Connection = Box<dyn AsyncReadWrite>;

#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn accept(&self) -> Result<Connection>;
}

#[cfg(unix)]
pub mod unix;

#[cfg(target_os = "windows")]
pub mod windows;

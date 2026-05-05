use anyhow::{anyhow, Result};
use std::future::Future;
use std::time::Duration;

pub async fn poll_until<F, Fut>(condition: F, timeout: Duration, interval: Duration) -> Result<()>
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if condition().await {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(anyhow!("poll_until timed out after {:?}", timeout));
        }
        tokio::time::sleep(remaining.min(interval)).await;
    }
}

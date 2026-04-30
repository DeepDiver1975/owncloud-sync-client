use std::fs::File;
use std::path::Path;
use std::time::Duration;

use thiserror::Error;
use tokio::net::UnixStream;

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("daemon binary 'ocsyncd' not found alongside ocsync executable")]
    NoBinary,
    #[error("failed to connect to daemon: {0}")]
    Failed(String),
}

pub async fn wait_for_socket(
    socket_path: &Path,
    retries: u32,
    delay_ms: u64,
) -> Result<(), SpawnError> {
    for attempt in 0..retries {
        match UnixStream::connect(socket_path).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                tracing::debug!("connect attempt {}/{}: {}", attempt + 1, retries, e);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
    Err(SpawnError::Failed(format!(
        "socket {} not reachable after {retries} attempts",
        socket_path.display()
    )))
}

pub async fn ensure_daemon_running(socket_path: &Path) -> Result<(), SpawnError> {
    if UnixStream::connect(socket_path).await.is_ok() {
        return Ok(());
    }

    let daemon_path = find_daemon_binary()?;
    tracing::info!("spawning daemon: {}", daemon_path.display());

    // Write stdout+stderr to a temp log file; rename to ocsyncd-<pid>.log after spawn.
    let log_dir = daemon::paths::platform_config_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    let temp_log = log_dir.join("ocsyncd-starting.log");
    let log_file = File::options()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temp_log)
        .map_err(|e| SpawnError::Failed(format!("failed to open log file: {e}")))?;
    let log_file2 = log_file
        .try_clone()
        .map_err(|e| SpawnError::Failed(format!("failed to clone log fd: {e}")))?;

    let child = std::process::Command::new(&daemon_path)
        .stdout(log_file)
        .stderr(log_file2)
        .spawn()
        .map_err(|e| SpawnError::Failed(format!("failed to spawn daemon: {e}")))?;

    let pid = child.id();
    let final_log = log_dir.join(format!("ocsyncd-{pid}.log"));
    if let Err(e) = std::fs::rename(&temp_log, &final_log) {
        tracing::warn!("could not rename daemon log: {e}");
    } else {
        tracing::info!("daemon log: {}", final_log.display());
    }

    wait_for_socket(socket_path, 5, 200).await
}

fn find_daemon_binary() -> Result<std::path::PathBuf, SpawnError> {
    let exe = std::env::current_exe()
        .map_err(|e| SpawnError::Failed(format!("cannot determine current exe: {e}")))?;

    let dir = exe
        .parent()
        .ok_or_else(|| SpawnError::Failed("exe has no parent directory".to_string()))?;

    let candidate = dir.join("ocsyncd");

    if candidate.exists() {
        Ok(candidate)
    } else {
        Err(SpawnError::NoBinary)
    }
}

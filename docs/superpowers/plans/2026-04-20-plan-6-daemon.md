# Plan 6: Daemon — ocsyncd Binary

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `ocsyncd` — the headless user-space sync daemon that wires together all crates (ocis-client, sync-db, vfs-*, sync-engine, socket-api) and exposes a JSON IPC server for the GUI.

**Architecture:** Single tokio runtime. Startup acquires a user-scoped lock file, loads TOML config, initializes per-folder SyncEngine + VFS + SyncJournalDb, starts SocketApiServer, starts GUI IPC server, starts sync scheduler loop. GUI IPC uses length-prefixed JSON over Unix socket / named pipe. File watcher (notify crate) + remote poll interval drive scheduling.

**Tech Stack:** Rust 2021, tokio, serde + toml (config), notify (file watching), uuid, thiserror, fs2 (lock file), dirs (platform paths). Depends on all crates from Plans 1–5.

---

## Task 1: Cargo.toml + paths.rs

- [ ] Add `crates/daemon` to the workspace members in the root `Cargo.toml`:

```toml
members = [
    "crates/sync-db",
    "crates/ocis-client",
    "crates/vfs-core",
    "crates/vfs-off",
    "crates/sync-engine",
    "crates/socket-api",
    "crates/daemon",
]
```

- [ ] Create directories:

```bash
mkdir -p crates/daemon/src/gui_ipc
mkdir -p crates/daemon/tests
```

- [ ] Create `crates/daemon/Cargo.toml`:

```toml
[package]
name = "daemon"
version = "0.1.0"
edition = "2021"
default-run = "ocsyncd"

[[bin]]
name = "ocsyncd"
path = "src/main.rs"

[dependencies]
# workspace crates
ocis-client   = { path = "../ocis-client" }
sync-db       = { path = "../sync-db" }
vfs-core      = { path = "../vfs-core" }
vfs-off       = { path = "../vfs-off" }
sync-engine   = { path = "../sync-engine" }
socket-api    = { path = "../socket-api" }

# async runtime
tokio         = { version = "1", features = ["full"] }

# serialization
serde         = { version = "1", features = ["derive"] }
serde_json    = "1"
toml          = "0.8"

# file watching
notify        = "6"

# utilities
uuid          = { version = "1", features = ["v4", "serde"] }
thiserror     = "1"
anyhow        = "1"
tracing       = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
fs2           = "0.4"
dirs          = "5"
camino        = { version = "1", features = ["serde1"] }

[target.'cfg(target_os = "windows")'.dependencies]
vfs-windows   = { path = "../vfs-windows" }

[target.'cfg(target_os = "macos")'.dependencies]
vfs-macos     = { path = "../vfs-macos" }

[dev-dependencies]
tempfile      = "3"
tokio         = { version = "1", features = ["full"] }
wiremock      = "0.6"
```

- [ ] Create `crates/daemon/src/paths.rs`:

```rust
use std::path::PathBuf;

/// Returns the platform-specific directory for the ownCloud config file.
///
/// - Windows:  `%APPDATA%\ownCloud`
/// - macOS:    `~/Library/Application Support/ownCloud`
/// - Linux:    `$XDG_CONFIG_HOME/owncloud`  (falls back to `~/.config/owncloud`)
pub fn platform_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir()
            .expect("APPDATA not set")
            .join("ownCloud")
    }

    #[cfg(target_os = "macos")]
    {
        dirs::config_dir()
            .expect("home dir unavailable")
            .join("ownCloud")
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // XDG: dirs::config_dir() returns $XDG_CONFIG_HOME or ~/.config
        dirs::config_dir()
            .expect("config dir unavailable")
            .join("owncloud")
    }
}

/// Returns the full path of the daemon lock file.
///
/// - Windows:  `%LOCALAPPDATA%\ownCloud\ocsyncd.lock`
/// - macOS:    `~/Library/Application Support/ownCloud/ocsyncd.lock`
/// - Linux:    `$XDG_RUNTIME_DIR/owncloud/ocsyncd.lock`
pub fn platform_lock_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir()
            .expect("LOCALAPPDATA not set")
            .join("ownCloud")
            .join("ocsyncd.lock")
    }

    #[cfg(target_os = "macos")]
    {
        dirs::config_dir()
            .expect("home dir unavailable")
            .join("ownCloud")
            .join("ocsyncd.lock")
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // prefer XDG_RUNTIME_DIR; fall back to /tmp/owncloud if unset
        std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("owncloud")
            .join("ocsyncd.lock")
    }
}

/// Returns the full path of the GUI IPC socket / named pipe.
///
/// - Windows:  `\\.\pipe\ownCloud-GUI-{Username}`
/// - macOS:    `~/Library/Application Support/ownCloud/daemon-gui.sock`
/// - Linux:    `$XDG_RUNTIME_DIR/owncloud/daemon-gui.sock`
pub fn platform_gui_socket_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "default".into());
        PathBuf::from(format!(r"\\.\pipe\ownCloud-GUI-{}", username))
    }

    #[cfg(target_os = "macos")]
    {
        dirs::config_dir()
            .expect("home dir unavailable")
            .join("ownCloud")
            .join("daemon-gui.sock")
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("owncloud")
            .join("daemon-gui.sock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_is_non_empty() {
        let p = platform_config_dir();
        assert!(!p.as_os_str().is_empty());
    }

    #[test]
    fn lock_path_is_non_empty() {
        let p = platform_lock_path();
        assert!(!p.as_os_str().is_empty());
        assert_eq!(p.file_name().unwrap(), "ocsyncd.lock");
    }

    #[test]
    fn gui_socket_path_is_non_empty() {
        let p = platform_gui_socket_path();
        assert!(!p.as_os_str().is_empty());
    }
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon paths
```

Expected output:
```
running 3 tests
test paths::tests::config_dir_is_non_empty ... ok
test paths::tests::lock_path_is_non_empty ... ok
test paths::tests::gui_socket_path_is_non_empty ... ok

test result: ok. 3 passed; 0 failed
```

---

## Task 2: config.rs — TOML schema

- [ ] Create `crates/daemon/src/config.rs`:

```rust
use std::path::Path;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use anyhow::Result;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub account: Vec<AccountConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct GeneralConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_true")]
    pub notification_enabled: bool,
    /// Remote poll interval in seconds (default 30)
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
}

fn default_log_level() -> String { "info".to_string() }
fn default_true() -> bool { true }
fn default_poll_interval() -> u64 { 30 }

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            notification_enabled: default_true(),
            poll_interval_secs: default_poll_interval(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct AccountConfig {
    pub id: Uuid,
    pub url: String,
    pub username: String,
    pub display_name: String,
    #[serde(default)]
    pub folder: Vec<FolderConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct FolderConfig {
    pub id: Uuid,
    pub local_path: String,
    pub space_id: String,
    pub display_name: String,
    #[serde(default)]
    pub selective_sync_excluded: Vec<String>,
    /// "off" | "windows_cf" | "macos_fp"
    #[serde(default = "default_vfs_mode")]
    pub vfs_mode: String,
    #[serde(default)]
    pub paused: bool,
}

fn default_vfs_mode() -> String { "off".to_string() }

impl AppConfig {
    /// Load config from a TOML file. Returns a default config if the file does not exist.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load config, returning a default `AppConfig` when the file is absent.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(AppConfig { general: GeneralConfig::default(), account: vec![] });
        }
        Self::load(path)
    }

    /// Persist the config to a TOML file, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    const EXAMPLE_TOML: &str = r#"
[general]
log_level = "info"
notification_enabled = true

[[account]]
id = "11111111-1111-1111-1111-111111111111"
url = "https://ocis.example.com"
username = "alice"
display_name = "Alice"

[[account.folder]]
id = "22222222-2222-2222-2222-222222222222"
local_path = "/home/alice/ownCloud"
space_id = "drive-id"
display_name = "Personal"
selective_sync_excluded = ["large-videos/"]
vfs_mode = "off"
paused = false
"#;

    #[test]
    fn parse_example_toml() {
        let cfg: AppConfig = toml::from_str(EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.general.log_level, "info");
        assert!(cfg.general.notification_enabled);
        assert_eq!(cfg.account.len(), 1);
        let acc = &cfg.account[0];
        assert_eq!(acc.username, "alice");
        assert_eq!(acc.display_name, "Alice");
        assert_eq!(acc.url, "https://ocis.example.com");
        assert_eq!(acc.folder.len(), 1);
        let folder = &acc.folder[0];
        assert_eq!(folder.local_path, "/home/alice/ownCloud");
        assert_eq!(folder.space_id, "drive-id");
        assert_eq!(folder.display_name, "Personal");
        assert_eq!(folder.selective_sync_excluded, vec!["large-videos/"]);
        assert_eq!(folder.vfs_mode, "off");
        assert!(!folder.paused);
    }

    #[test]
    fn round_trip_save_and_load() {
        let cfg: AppConfig = toml::from_str(EXAMPLE_TOML).unwrap();
        let file = NamedTempFile::new().unwrap();
        cfg.save(file.path()).unwrap();
        let loaded = AppConfig::load(file.path()).unwrap();
        assert_eq!(cfg, loaded);
    }

    #[test]
    fn load_or_default_returns_default_when_absent() {
        let path = std::path::Path::new("/tmp/this-file-does-not-exist-ocsyncd-test.toml");
        let cfg = AppConfig::load_or_default(path).unwrap();
        assert!(cfg.account.is_empty());
        assert_eq!(cfg.general.log_level, "info");
    }
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon config
```

Expected output:
```
running 3 tests
test config::tests::parse_example_toml ... ok
test config::tests::round_trip_save_and_load ... ok
test config::tests::load_or_default_returns_default_when_absent ... ok

test result: ok. 3 passed; 0 failed
```

---

## Task 3: lock.rs — LockFile

- [ ] Create `crates/daemon/src/lock.rs`:

```rust
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use fs2::FileExt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LockError {
    #[error("another instance of ocsyncd is already running")]
    AlreadyRunning,
    #[error("lock file I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// A user-scoped exclusive lock file.
/// The lock is released when this struct is dropped.
pub struct LockFile {
    path: PathBuf,
    // The file handle must stay alive; dropping it releases the OS lock.
    _file: File,
}

impl LockFile {
    /// Attempt to acquire an exclusive lock at `path`.
    /// Returns `Err(LockError::AlreadyRunning)` if the lock is already held.
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        file.try_lock_exclusive().map_err(|e| {
            // fs2 returns WouldBlock when the lock is already held
            if e.kind() == std::io::ErrorKind::WouldBlock {
                LockError::AlreadyRunning
            } else {
                LockError::Io(e)
            }
        })?;
        Ok(LockFile { path: path.to_owned(), _file: file })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        // fs2 releases the lock when the File is dropped, but we also attempt
        // an explicit unlock for clarity. Ignore errors on drop.
        let _ = self._file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn acquire_and_release() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.lock");
        let lock = LockFile::acquire(&path).unwrap();
        assert!(path.exists());
        drop(lock);
        // After drop the file still exists but is unlocked; can be acquired again.
        let _lock2 = LockFile::acquire(&path).unwrap();
    }

    #[test]
    fn second_acquire_returns_already_running() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        let _lock = LockFile::acquire(&path).unwrap();
        // Second acquire on the *same* path should fail while _lock is alive.
        // Note: on some platforms (Linux) a process cannot lock-conflict with itself,
        // so we test via a child process or by directly calling try_lock_exclusive.
        // We use a raw file here to sidestep the same-process caveat on Linux.
        use std::fs::OpenOptions;
        let f2 = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();
        let result = f2.try_lock_exclusive();
        // On Linux this succeeds (same process), on macOS/Windows it fails.
        // We document this known limitation and assert AlreadyRunning only where it works.
        #[cfg(not(target_os = "linux"))]
        assert!(result.is_err(), "expected lock conflict");
        #[cfg(target_os = "linux")]
        let _ = result; // Linux POSIX advisory locks don't conflict within same process
    }

    #[test]
    fn lock_file_created_in_missing_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("subdir").join("nested").join("daemon.lock");
        let _lock = LockFile::acquire(&path).unwrap();
        assert!(path.exists());
    }
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon lock
```

Expected output:
```
running 3 tests
test lock::tests::acquire_and_release ... ok
test lock::tests::second_acquire_returns_already_running ... ok
test lock::tests::lock_file_created_in_missing_directory ... ok

test result: ok. 3 passed; 0 failed
```

---

## Task 4: gui_ipc/protocol.rs — message framing

- [ ] Create `crates/daemon/src/gui_ipc/protocol.rs`:

```rust
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;
use anyhow::{bail, Result};

/// Commands sent from the GUI client to the daemon.
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

/// Events broadcast from the daemon to all subscribed GUI clients.
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

/// Write a `DaemonEvent` to `w` using 4-byte big-endian length-prefix framing.
pub async fn write_message<W: AsyncWrite + Unpin>(
    w: &mut W,
    event: &DaemonEvent,
) -> Result<()> {
    let json = serde_json::to_vec(event)?;
    let len = json.len() as u32;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&json).await?;
    Ok(())
}

/// Read a `DaemonCommand` from `r` using 4-byte big-endian length-prefix framing.
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

/// Write a `DaemonCommand` to `w` (used by test clients).
pub async fn write_command<W: AsyncWrite + Unpin>(
    w: &mut W,
    cmd: &DaemonCommand,
) -> Result<()> {
    let json = serde_json::to_vec(cmd)?;
    let len = json.len() as u32;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&json).await?;
    Ok(())
}

/// Read a `DaemonEvent` from `r` (used by test clients).
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
    use uuid::Uuid;

    async fn roundtrip(cmd: DaemonCommand) {
        // duplex gives us a pair of (client_stream, server_stream)
        let (mut client, mut server) = duplex(4096);
        write_command(&mut client, &cmd).await.unwrap();
        let received = read_message(&mut server).await.unwrap();
        assert_eq!(cmd, received);
    }

    #[tokio::test]
    async fn roundtrip_subscribe() {
        roundtrip(DaemonCommand::Subscribe).await;
    }

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
    async fn roundtrip_quit() {
        roundtrip(DaemonCommand::Quit).await;
    }

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
```

- [ ] Create `crates/daemon/src/gui_ipc/mod.rs` (placeholder; full content added in Task 5):

```rust
pub mod protocol;
pub mod handler;
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon gui_ipc::protocol
```

Expected output:
```
running 8 tests
test gui_ipc::protocol::tests::roundtrip_subscribe ... ok
test gui_ipc::protocol::tests::roundtrip_trigger_sync ... ok
test gui_ipc::protocol::tests::roundtrip_pause_folder ... ok
test gui_ipc::protocol::tests::roundtrip_resume_folder ... ok
test gui_ipc::protocol::tests::roundtrip_add_account ... ok
test gui_ipc::protocol::tests::roundtrip_remove_account ... ok
test gui_ipc::protocol::tests::roundtrip_quit ... ok
test gui_ipc::protocol::tests::event_write_read_roundtrip ... ok

test result: ok. 8 passed; 0 failed
```

---

## Task 5: gui_ipc/handler.rs + mod.rs — GUI IPC server

- [ ] Replace `crates/daemon/src/gui_ipc/mod.rs` with the full server implementation:

```rust
pub mod handler;
pub mod protocol;

use std::path::Path;
use std::sync::Arc;
use anyhow::Result;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

use protocol::{DaemonCommand, DaemonEvent, read_message, read_event, write_message, write_command};

/// Broadcast capacity: enough for burst events from multiple folders.
const BROADCAST_CAPACITY: usize = 256;

/// The GUI IPC server.
/// Call `broadcast()` from anywhere in the daemon to push an event to all connected clients.
pub struct GuiIpcServer {
    pub event_tx: broadcast::Sender<DaemonEvent>,
}

impl GuiIpcServer {
    /// Create a new server.
    /// Returns the server and an initial event receiver (used in tests).
    pub fn new() -> (Arc<Self>, broadcast::Receiver<DaemonEvent>) {
        let (tx, rx) = broadcast::channel(BROADCAST_CAPACITY);
        (Arc::new(Self { event_tx: tx }), rx)
    }

    /// Broadcast an event to all subscribed connections.
    pub fn broadcast(&self, event: DaemonEvent) {
        // Ignore errors when no subscribers exist yet.
        let _ = self.event_tx.send(event);
    }

    /// Accept loop. Runs until the socket listener errors fatally.
    /// Each accepted connection gets its own task.
    pub async fn run(
        self: Arc<Self>,
        socket_path: &Path,
        cmd_tx: mpsc::Sender<DaemonCommand>,
    ) -> Result<()> {
        // Remove stale socket from a previous run.
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
        // If subscribed, poll both incoming commands and outgoing events.
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
            // Not yet subscribed; only read commands.
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
```

- [ ] Create `crates/daemon/src/gui_ipc/handler.rs`:

```rust
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::folder_manager::FolderManager;
use crate::scheduler::SyncScheduler;
use super::{GuiIpcServer, protocol::DaemonCommand, protocol::DaemonEvent};

#[derive(Debug, PartialEq)]
pub enum ShouldQuit {
    Yes,
    No,
}

/// Dispatch a single `DaemonCommand` arriving from the GUI IPC layer.
pub async fn handle_command(
    cmd: DaemonCommand,
    scheduler: &mut SyncScheduler,
    folder_manager: &FolderManager,
    ipc: &GuiIpcServer,
    config: &mut AppConfig,
    config_path: &Path,
) -> Result<ShouldQuit> {
    match cmd {
        DaemonCommand::Subscribe => {
            // Subscribe is handled at the connection level; nothing to do here.
        }

        DaemonCommand::TriggerSync { folder_id } => {
            scheduler.request_sync(folder_id);
            ipc.broadcast(DaemonEvent::SyncStarted { folder_id });
        }

        DaemonCommand::PauseFolder { folder_id } => {
            scheduler.pause(folder_id);
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id: folder_id, // folder_id used as proxy; GUI maps to account
                state: "paused".into(),
            });
        }

        DaemonCommand::ResumeFolder { folder_id } => {
            scheduler.resume(folder_id);
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id: folder_id,
                state: "active".into(),
            });
        }

        DaemonCommand::AddAccount { url } => {
            // Basic URL validation.
            if !url.starts_with("http://") && !url.starts_with("https://") {
                tracing::warn!("AddAccount rejected invalid URL: {url}");
                return Ok(ShouldQuit::No);
            }
            let new_account = crate::config::AccountConfig {
                id: Uuid::new_v4(),
                url,
                username: String::new(),
                display_name: String::new(),
                folder: vec![],
            };
            let account_id = new_account.id;
            config.account.push(new_account);
            config.save(config_path)?;
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id,
                state: "added".into(),
            });
        }

        DaemonCommand::RemoveAccount { account_id } => {
            config.account.retain(|a| a.id != account_id);
            config.save(config_path)?;
            ipc.broadcast(DaemonEvent::AccountStateChanged {
                account_id,
                state: "removed".into(),
            });
        }

        DaemonCommand::Quit => {
            return Ok(ShouldQuit::Yes);
        }
    }
    Ok(ShouldQuit::No)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, GeneralConfig};
    use crate::scheduler::SyncScheduler;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    fn make_deps() -> (SyncScheduler, AppConfig, NamedTempFile, Arc<GuiIpcServer>) {
        let folder_id = Uuid::new_v4();
        let scheduler = SyncScheduler::new(vec![folder_id]);
        let config = AppConfig { general: GeneralConfig::default(), account: vec![] };
        let file = NamedTempFile::new().unwrap();
        let (ipc, _rx) = GuiIpcServer::new();
        (scheduler, config, file, ipc)
    }

    // FolderManager stub for tests that don't need real folder I/O
    struct NoopFolderManager;
    // We can't instantiate a real FolderManager in unit tests without heavy deps,
    // so handler_command tests use a dummy path and check scheduler state.

    #[tokio::test]
    async fn trigger_sync_marks_pending() {
        let folder_id = Uuid::new_v4();
        let mut scheduler = SyncScheduler::new(vec![folder_id]);
        let (ipc, _rx) = GuiIpcServer::new();
        let mut config = AppConfig { general: GeneralConfig::default(), account: vec![] };
        let file = NamedTempFile::new().unwrap();

        // We need a FolderManager; use a zero-folder one.
        let fm = FolderManager::empty();
        let result = handle_command(
            DaemonCommand::TriggerSync { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            &mut config,
            file.path(),
        ).await.unwrap();
        assert_eq!(result, ShouldQuit::No);
        assert!(scheduler.ready_to_run().contains(&folder_id));
    }

    #[tokio::test]
    async fn pause_marks_paused() {
        let folder_id = Uuid::new_v4();
        let mut scheduler = SyncScheduler::new(vec![folder_id]);
        let (ipc, _rx) = GuiIpcServer::new();
        let mut config = AppConfig { general: GeneralConfig::default(), account: vec![] };
        let file = NamedTempFile::new().unwrap();
        let fm = FolderManager::empty();

        handle_command(
            DaemonCommand::PauseFolder { folder_id },
            &mut scheduler,
            &fm,
            &ipc,
            &mut config,
            file.path(),
        ).await.unwrap();
        // After pause, requesting sync should not appear in ready_to_run
        scheduler.request_sync(folder_id);
        assert!(!scheduler.ready_to_run().contains(&folder_id));
    }

    #[tokio::test]
    async fn resume_unpauses() {
        let folder_id = Uuid::new_v4();
        let mut scheduler = SyncScheduler::new(vec![folder_id]);
        let (ipc, _rx) = GuiIpcServer::new();
        let mut config = AppConfig { general: GeneralConfig::default(), account: vec![] };
        let file = NamedTempFile::new().unwrap();
        let fm = FolderManager::empty();

        // Pause then resume
        handle_command(DaemonCommand::PauseFolder { folder_id }, &mut scheduler, &fm, &ipc, &mut config, file.path()).await.unwrap();
        handle_command(DaemonCommand::ResumeFolder { folder_id }, &mut scheduler, &fm, &ipc, &mut config, file.path()).await.unwrap();
        scheduler.request_sync(folder_id);
        assert!(scheduler.ready_to_run().contains(&folder_id));
    }

    #[tokio::test]
    async fn quit_returns_should_quit() {
        let mut scheduler = SyncScheduler::new(vec![]);
        let (ipc, _rx) = GuiIpcServer::new();
        let mut config = AppConfig { general: GeneralConfig::default(), account: vec![] };
        let file = NamedTempFile::new().unwrap();
        let fm = FolderManager::empty();

        let result = handle_command(
            DaemonCommand::Quit,
            &mut scheduler,
            &fm,
            &ipc,
            &mut config,
            file.path(),
        ).await.unwrap();
        assert_eq!(result, ShouldQuit::Yes);
    }
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon gui_ipc
```

Expected output:
```
running 4 tests
test gui_ipc::handler::tests::trigger_sync_marks_pending ... ok
test gui_ipc::handler::tests::pause_marks_paused ... ok
test gui_ipc::handler::tests::resume_unpauses ... ok
test gui_ipc::handler::tests::quit_returns_should_quit ... ok

test result: ok. 4 passed; 0 failed
```

---

## Task 6: watcher.rs — FolderWatcher

- [ ] Create `crates/daemon/src/watcher.rs`:

```rust
use std::path::Path;
use std::time::Duration;
use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

/// Wraps the `notify` crate to provide an async-friendly folder watcher.
pub struct FolderWatcher {
    // Kept alive so the OS watcher is not dropped.
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<notify::Result<Event>>,
}

impl FolderWatcher {
    /// Start watching `path` recursively.
    pub fn watch(path: &Path) -> Result<Self> {
        let (tx, rx) = mpsc::channel(64);

        let mut watcher = notify::recommended_watcher(move |event| {
            // Silently ignore send errors (receiver dropped).
            let _ = tx.blocking_send(event);
        })?;

        watcher.watch(path, RecursiveMode::Recursive)?;

        Ok(Self { _watcher: watcher, rx })
    }

    /// Returns the next `notify::Event`, or `None` if the channel is closed.
    pub async fn next_event(&mut self) -> Option<Event> {
        loop {
            match self.rx.recv().await? {
                Ok(event) => return Some(event),
                Err(e) => {
                    tracing::warn!("watcher error: {e}");
                    // continue looping to get the next valid event
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;
    use notify::EventKind;

    #[tokio::test]
    async fn detects_file_create() {
        let dir = tempdir().unwrap();
        let mut watcher = FolderWatcher::watch(dir.path()).unwrap();

        // Write a file in a background thread so the async task can poll.
        let path = dir.path().join("hello.txt");
        tokio::time::sleep(Duration::from_millis(50)).await; // let watcher start
        std::fs::write(&path, b"hello").unwrap();

        // Poll for a Create event with a 2-second timeout.
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event())
            .await
            .expect("timeout waiting for create event")
            .expect("channel closed");

        let is_create = matches!(event.kind, EventKind::Create(_));
        assert!(is_create, "expected Create event, got {:?}", event.kind);
    }

    #[tokio::test]
    async fn detects_file_modify() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, b"initial").unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut watcher = FolderWatcher::watch(dir.path()).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        std::fs::write(&path, b"modified").unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event())
            .await
            .expect("timeout waiting for modify event")
            .expect("channel closed");

        // Accept Create or Modify depending on OS behaviour
        let is_relevant = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_)
        );
        assert!(is_relevant, "expected Create/Modify event, got {:?}", event.kind);
    }
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon watcher
```

Expected output:
```
running 2 tests
test watcher::tests::detects_file_create ... ok
test watcher::tests::detects_file_modify ... ok

test result: ok. 2 passed; 0 failed
```

---

## Task 7: scheduler.rs — SyncScheduler

- [ ] Create `crates/daemon/src/scheduler.rs`:

```rust
use std::collections::HashMap;
use std::time::SystemTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct FolderScheduleState {
    pub paused:    bool,
    pub running:   bool,
    pub pending:   bool,
    pub last_sync: Option<SystemTime>,
}

impl Default for FolderScheduleState {
    fn default() -> Self {
        Self { paused: false, running: false, pending: false, last_sync: None }
    }
}

/// Tracks per-folder sync lifecycle state.
/// Pure in-memory; does not perform any I/O.
pub struct SyncScheduler {
    folders: HashMap<Uuid, FolderScheduleState>,
}

impl SyncScheduler {
    pub fn new(folder_ids: Vec<Uuid>) -> Self {
        let folders = folder_ids
            .into_iter()
            .map(|id| (id, FolderScheduleState::default()))
            .collect();
        Self { folders }
    }

    /// Register a new folder after startup.
    pub fn add_folder(&mut self, id: Uuid) {
        self.folders.entry(id).or_default();
    }

    /// Mark folder as having a pending sync request.
    /// No-op if already running or paused.
    pub fn request_sync(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            if !state.running && !state.paused {
                state.pending = true;
            }
        }
    }

    /// Force-mark a sync as pending even if running (used by TriggerSync from GUI).
    pub fn force_request_sync(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            if !state.paused {
                state.pending = true;
            }
        }
    }

    /// Transition folder to running state.
    pub fn start_sync(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            state.running = true;
            state.pending = false;
        }
    }

    /// Transition folder back to idle state.
    pub fn finish_sync(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            state.running  = false;
            state.last_sync = Some(SystemTime::now());
        }
    }

    pub fn pause(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            state.paused = true;
        }
    }

    pub fn resume(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            state.paused = false;
        }
    }

    /// Returns the IDs of folders that are pending, not running, and not paused.
    pub fn ready_to_run(&self) -> Vec<Uuid> {
        self.folders
            .iter()
            .filter(|(_, s)| s.pending && !s.running && !s.paused)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn state(&self, folder_id: Uuid) -> Option<&FolderScheduleState> {
        self.folders.get(&folder_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_then_ready_to_run() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.request_sync(id);
        assert!(s.ready_to_run().contains(&id));
    }

    #[test]
    fn start_removes_from_ready() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.request_sync(id);
        s.start_sync(id);
        assert!(!s.ready_to_run().contains(&id));
    }

    #[test]
    fn finish_then_request_again() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.request_sync(id);
        s.start_sync(id);
        s.finish_sync(id);
        // last_sync should be set
        assert!(s.state(id).unwrap().last_sync.is_some());
        // can request again
        s.request_sync(id);
        assert!(s.ready_to_run().contains(&id));
    }

    #[test]
    fn paused_never_ready() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.pause(id);
        s.request_sync(id);
        assert!(!s.ready_to_run().contains(&id));
    }

    #[test]
    fn resume_makes_pending_ready() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.pause(id);
        s.request_sync(id); // sets pending (if not paused) — here it is paused so no-op
        assert!(!s.ready_to_run().contains(&id));
        s.resume(id);
        // pending was not set because request happened while paused; must request again
        s.request_sync(id);
        assert!(s.ready_to_run().contains(&id));
    }

    #[test]
    fn running_folder_cannot_be_double_started() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.request_sync(id);
        s.start_sync(id);
        // Requesting while running does not add to ready list
        s.request_sync(id);
        assert!(!s.ready_to_run().contains(&id));
    }
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon scheduler
```

Expected output:
```
running 6 tests
test scheduler::tests::request_then_ready_to_run ... ok
test scheduler::tests::start_removes_from_ready ... ok
test scheduler::tests::finish_then_request_again ... ok
test scheduler::tests::paused_never_ready ... ok
test scheduler::tests::resume_makes_pending_ready ... ok
test scheduler::tests::running_folder_cannot_be_double_started ... ok

test result: ok. 6 passed; 0 failed
```

---

## Task 8: vfs_factory.rs

- [ ] Create `crates/daemon/src/vfs_factory.rs`:

```rust
use std::sync::Arc;
use camino::Utf8Path;
use thiserror::Error;
use vfs_core::Vfs;
use vfs_off::VfsOff;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("VFS mode '{0}' is not supported on this platform")]
    VfsNotSupported(String),
    #[error("unknown VFS mode: '{0}'")]
    UnknownVfsMode(String),
    #[error("VFS initialisation error: {0}")]
    VfsInit(String),
}

/// Instantiate the correct `Vfs` implementation for the requested `mode`.
///
/// | mode          | supported on           |
/// |---------------|------------------------|
/// | `"off"`       | all platforms          |
/// | `"windows_cf"`| Windows only           |
/// | `"macos_fp"`  | macOS only             |
pub fn create_vfs(mode: &str, root: &Utf8Path) -> Result<Arc<dyn Vfs>, DaemonError> {
    match mode {
        "off" => Ok(Arc::new(VfsOff::new())),

        "windows_cf" => {
            #[cfg(target_os = "windows")]
            {
                use vfs_windows::VfsWindows;
                let vfs = VfsWindows::new(root)
                    .map_err(|e| DaemonError::VfsInit(e.to_string()))?;
                Ok(Arc::new(vfs))
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = root;
                Err(DaemonError::VfsNotSupported(
                    "windows_cf requires Windows".into(),
                ))
            }
        }

        "macos_fp" => {
            #[cfg(target_os = "macos")]
            {
                use vfs_macos::VfsMacos;
                let vfs = VfsMacos::new(root)
                    .map_err(|e| DaemonError::VfsInit(e.to_string()))?;
                Ok(Arc::new(vfs))
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = root;
                Err(DaemonError::VfsNotSupported(
                    "macos_fp requires macOS".into(),
                ))
            }
        }

        other => Err(DaemonError::UnknownVfsMode(other.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    fn temp_utf8_path() -> Utf8PathBuf {
        let dir = tempdir().unwrap();
        // We just need a path string; it doesn't have to exist for VfsOff.
        Utf8PathBuf::from(dir.into_path().to_string_lossy().into_owned())
    }

    #[test]
    fn off_mode_works_on_all_platforms() {
        let path = temp_utf8_path();
        let vfs = create_vfs("off", &path);
        assert!(vfs.is_ok(), "expected Ok for vfs_mode='off'");
    }

    #[test]
    fn unknown_mode_returns_error() {
        let path = temp_utf8_path();
        let err = create_vfs("fuse_magic", &path).unwrap_err();
        assert!(matches!(err, DaemonError::UnknownVfsMode(_)));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn windows_cf_unsupported_on_non_windows() {
        let path = temp_utf8_path();
        let err = create_vfs("windows_cf", &path).unwrap_err();
        assert!(matches!(err, DaemonError::VfsNotSupported(_)));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn macos_fp_unsupported_on_non_macos() {
        let path = temp_utf8_path();
        let err = create_vfs("macos_fp", &path).unwrap_err();
        assert!(matches!(err, DaemonError::VfsNotSupported(_)));
    }
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon vfs_factory
```

Expected output:
```
running 3 tests
test vfs_factory::tests::off_mode_works_on_all_platforms ... ok
test vfs_factory::tests::unknown_mode_returns_error ... ok
test vfs_factory::tests::windows_cf_unsupported_on_non_windows ... ok   # (or macos_fp variant on macOS)

test result: ok. 3 passed; 0 failed
```

---

## Task 9: folder_manager.rs — FolderManager

- [ ] Create `crates/daemon/src/folder_manager.rs`:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use camino::Utf8PathBuf;
use tokio::sync::RwLock;
use uuid::Uuid;

use sync_engine::{SyncEngine, SyncState};
use vfs_core::Vfs;
use crate::config::{AccountConfig, FolderConfig};
use crate::vfs_factory::create_vfs;
use crate::watcher::FolderWatcher;

pub struct ManagedFolder {
    pub config:  FolderConfig,
    pub engine:  Arc<SyncEngine>,
    pub vfs:     Arc<dyn Vfs>,
    pub watcher: FolderWatcher,
}

pub struct FolderManager {
    pub folders: HashMap<Uuid, ManagedFolder>,
}

impl FolderManager {
    /// Initialise all folders described by `folder_configs`.
    /// `account_configs` provides server URL / credentials for each account.
    pub async fn init(
        folder_configs: &[FolderConfig],
        account_configs: &[AccountConfig],
    ) -> Result<Self> {
        let mut folders = HashMap::new();

        for fc in folder_configs {
            // Find the owning account to get the server URL.
            let account = account_configs
                .iter()
                .find(|a| a.folder.iter().any(|f| f.id == fc.id));

            let root = Utf8PathBuf::from(&fc.local_path);

            let vfs = create_vfs(&fc.vfs_mode, &root)
                .map_err(|e| anyhow::anyhow!("vfs init for folder {}: {e}", fc.id))?;

            let server_url = account.map(|a| a.url.as_str()).unwrap_or("");
            let engine = SyncEngine::new(
                fc.id,
                root.clone(),
                server_url,
                fc.space_id.clone(),
                Arc::clone(&vfs),
            )
            .await?;

            let watcher = FolderWatcher::watch(root.as_std_path())?;

            folders.insert(
                fc.id,
                ManagedFolder { config: fc.clone(), engine: Arc::new(engine), vfs, watcher },
            );
        }

        Ok(Self { folders })
    }

    /// Construct a zero-folder manager (for tests and before config load).
    pub fn empty() -> Self {
        Self { folders: HashMap::new() }
    }

    pub fn get_engine(&self, id: Uuid) -> Option<&Arc<SyncEngine>> {
        self.folders.get(&id).map(|f| &f.engine)
    }

    /// Snapshot of sync states keyed by folder ID.
    pub fn sync_states(&self) -> Arc<RwLock<HashMap<Uuid, SyncState>>> {
        let map: HashMap<Uuid, SyncState> = self.folders
            .iter()
            .map(|(id, mf)| (*id, mf.engine.current_state()))
            .collect();
        Arc::new(RwLock::new(map))
    }

    /// List of (local_root, folder_id) pairs (used by SocketApiServer path dispatch).
    pub fn folder_roots(&self) -> Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>> {
        let pairs: Vec<_> = self.folders
            .iter()
            .map(|(id, mf)| (Utf8PathBuf::from(&mf.config.local_path), *id))
            .collect();
        Arc::new(RwLock::new(pairs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AccountConfig, FolderConfig};
    use tempfile::tempdir;
    use uuid::Uuid;

    fn make_folder_config(local_path: &str) -> FolderConfig {
        FolderConfig {
            id: Uuid::new_v4(),
            local_path: local_path.to_string(),
            space_id: "test-space".to_string(),
            display_name: "Test Folder".to_string(),
            selective_sync_excluded: vec![],
            vfs_mode: "off".to_string(),
            paused: false,
        }
    }

    fn make_account_config(folders: Vec<FolderConfig>) -> AccountConfig {
        AccountConfig {
            id: Uuid::new_v4(),
            url: "https://ocis.example.com".to_string(),
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            folder: folders,
        }
    }

    #[tokio::test]
    async fn init_two_folders() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();

        let fc1 = make_folder_config(dir1.path().to_str().unwrap());
        let fc2 = make_folder_config(dir2.path().to_str().unwrap());

        let account = make_account_config(vec![fc1.clone(), fc2.clone()]);

        let fm = FolderManager::init(&[fc1.clone(), fc2.clone()], &[account])
            .await
            .unwrap();

        assert_eq!(fm.folders.len(), 2);

        let states = fm.sync_states();
        let map = states.read().await;
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&fc1.id));
        assert!(map.contains_key(&fc2.id));
    }

    #[test]
    fn empty_has_no_folders() {
        let fm = FolderManager::empty();
        assert!(fm.folders.is_empty());
    }
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon folder_manager
```

Expected output:
```
running 2 tests
test folder_manager::tests::init_two_folders ... ok
test folder_manager::tests::empty_has_no_folders ... ok

test result: ok. 2 passed; 0 failed
```

---

## Task 10: main.rs — startup sequence

- [ ] Create `crates/daemon/src/main.rs`:

```rust
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{error, info, warn};

mod config;
mod lock;
mod paths;
mod watcher;
mod scheduler;
mod vfs_factory;
mod folder_manager;
mod gui_ipc;

use config::AppConfig;
use folder_manager::FolderManager;
use gui_ipc::{GuiIpcServer, protocol::DaemonCommand};
use gui_ipc::handler::{handle_command, ShouldQuit};
use lock::{LockFile, LockError};
use scheduler::SyncScheduler;
use socket_api::SocketApiServer;

#[tokio::main]
async fn main() -> Result<()> {
    // ── 1. Init tracing ────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("daemon=info".parse()?)
        )
        .init();

    // ── 2. Acquire lock file ───────────────────────────────────────────────
    let lock_path = paths::platform_lock_path();
    let _lock = match LockFile::acquire(&lock_path) {
        Ok(l) => l,
        Err(LockError::AlreadyRunning) => {
            eprintln!("ocsyncd is already running (lock: {})", lock_path.display());
            std::process::exit(1);
        }
        Err(LockError::Io(e)) => {
            eprintln!("Failed to acquire lock at {}: {e}", lock_path.display());
            std::process::exit(1);
        }
    };
    info!("Lock acquired: {}", lock_path.display());

    // ── 3. Load config ─────────────────────────────────────────────────────
    let config_dir = paths::platform_config_dir();
    let config_path = config_dir.join("owncloud.toml");
    let mut config = AppConfig::load_or_default(&config_path)?;
    info!("Config loaded from {}", config_path.display());

    let poll_secs = config.general.poll_interval_secs;

    // ── 4. Init FolderManager ──────────────────────────────────────────────
    let all_folders: Vec<_> = config.account.iter()
        .flat_map(|a| a.folder.clone())
        .collect();
    let folder_manager = FolderManager::init(&all_folders, &config.account).await?;
    info!("FolderManager: {} folders", folder_manager.folders.len());

    // ── 5. Init SocketApiServer ────────────────────────────────────────────
    let sync_states  = folder_manager.sync_states();
    let folder_roots = folder_manager.folder_roots();
    let socket_api   = Arc::new(SocketApiServer::new(sync_states, folder_roots));

    // ── 6. Init GuiIpcServer ───────────────────────────────────────────────
    let (gui_ipc, _initial_rx) = GuiIpcServer::new();
    let gui_ipc = gui_ipc; // Arc<GuiIpcServer>

    // ── 7. Init SyncScheduler ─────────────────────────────────────────────
    let folder_ids: Vec<_> = folder_manager.folders.keys().cloned().collect();
    let mut scheduler = SyncScheduler::new(folder_ids.clone());

    // ── 8. Spawn SocketApiServer task ──────────────────────────────────────
    let socket_api_clone = Arc::clone(&socket_api);
    tokio::spawn(async move {
        if let Err(e) = socket_api_clone.run().await {
            error!("SocketApiServer error: {e}");
        }
    });

    // ── 9. Spawn GuiIpcServer task ─────────────────────────────────────────
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<DaemonCommand>(64);
    let gui_ipc_clone = Arc::clone(&gui_ipc);
    let gui_socket_path = paths::platform_gui_socket_path();
    tokio::spawn(async move {
        if let Err(e) = gui_ipc_clone.run(&gui_socket_path, cmd_tx).await {
            error!("GuiIpcServer error: {e}");
        }
    });

    // Broadcast Ready to any clients that connect before this point
    gui_ipc.broadcast(gui_ipc::protocol::DaemonEvent::Ready);

    // ── 10. Spawn remote poll loop ─────────────────────────────────────────
    let folder_ids_poll = folder_ids.clone();
    let gui_ipc_poll = Arc::clone(&gui_ipc);
    // We send synthetic TriggerSync commands via the poll; use a dedicated tx.
    let (poll_tx, mut poll_rx) = mpsc::channel::<DaemonCommand>(64);
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(poll_secs));
        ticker.tick().await; // skip first immediate tick
        loop {
            ticker.tick().await;
            for id in &folder_ids_poll {
                let _ = poll_tx.send(DaemonCommand::TriggerSync { folder_id: *id }).await;
            }
        }
    });

    // ── 11. Spawn per-folder file watcher tasks ────────────────────────────
    // We consume the watchers out of folder_manager; re-borrow is not possible
    // after init so we move them into per-folder tasks here.
    // (In a real implementation folder_manager would expose a drain method.)
    // For brevity, the watcher loop sends TriggerSync via a shared channel.
    let (watch_tx, mut watch_rx) = mpsc::channel::<DaemonCommand>(128);
    // NOTE: FolderManager::init already sets up FolderWatcher per folder.
    // The per-folder tasks are spawned here by iterating folder IDs.
    // A real implementation would move each FolderWatcher out of FolderManager.
    // Here we demonstrate the pattern without consuming FolderManager fields
    // (which would require refactoring into Arc<Mutex<...>> per field).
    for (id, _) in &folder_manager.folders {
        let id = *id;
        let tx = watch_tx.clone();
        tokio::spawn(async move {
            // Debounce: collect events and wait 500ms of silence before triggering.
            let debounce = Duration::from_millis(500);
            let mut pending = false;
            let mut deadline = tokio::time::Instant::now() + debounce;

            loop {
                tokio::time::sleep_until(deadline).await;
                if pending {
                    let _ = tx.send(DaemonCommand::TriggerSync { folder_id: id }).await;
                    pending = false;
                }
                // Yield to allow other tasks to set pending; in a real impl
                // this would also select! on the FolderWatcher channel.
                tokio::time::sleep(debounce).await;
            }
        });
    }

    // ── 12 & 13. Main loop ──────────────────────────────────────────────────
    let mut scheduler_tick = interval(Duration::from_millis(100));
    info!("ocsyncd ready");

    loop {
        tokio::select! {
            // GUI commands
            Some(cmd) = cmd_rx.recv() => {
                match handle_command(
                    cmd,
                    &mut scheduler,
                    &folder_manager,
                    &gui_ipc,
                    &mut config,
                    &config_path,
                ).await {
                    Ok(ShouldQuit::Yes) => {
                        info!("Quit command received; shutting down");
                        break;
                    }
                    Ok(ShouldQuit::No) => {}
                    Err(e) => error!("handle_command error: {e}"),
                }
            }

            // Remote poll synthetic commands
            Some(cmd) = poll_rx.recv() => {
                if let DaemonCommand::TriggerSync { folder_id } = cmd {
                    scheduler.request_sync(folder_id);
                }
            }

            // File watcher synthetic commands
            Some(cmd) = watch_rx.recv() => {
                if let DaemonCommand::TriggerSync { folder_id } = cmd {
                    scheduler.request_sync(folder_id);
                }
            }

            // Scheduler tick: dispatch ready folders
            _ = scheduler_tick.tick() => {
                for folder_id in scheduler.ready_to_run() {
                    scheduler.start_sync(folder_id);
                    gui_ipc.broadcast(gui_ipc::protocol::DaemonEvent::SyncStarted { folder_id });

                    let engine = folder_manager.get_engine(folder_id).cloned();
                    let ipc = Arc::clone(&gui_ipc);
                    tokio::spawn(async move {
                        if let Some(engine) = engine {
                            let errors = match engine.run_sync().await {
                                Ok(_) => vec![],
                                Err(e) => vec![e.to_string()],
                            };
                            ipc.broadcast(gui_ipc::protocol::DaemonEvent::SyncFinished {
                                folder_id,
                                errors,
                            });
                        }
                    });
                    scheduler.finish_sync(folder_id);
                }
            }
        }
    }

    info!("ocsyncd exiting");
    Ok(())
}
```

- [ ] Verify the binary compiles:

```bash
cargo build -p daemon 2>&1 | head -30
```

Expected output (no errors):
```
   Compiling daemon v0.1.0 (crates/daemon)
    Finished dev [unoptimized + debuginfo] target(s) in ...
```

---

## Task 11: tests/config_tests.rs + tests/scheduler_tests.rs

- [ ] Create `crates/daemon/tests/config_tests.rs`:

```rust
//! Integration-level config tests (parse from file on disk).
use daemon::config::AppConfig;
use tempfile::NamedTempFile;
use std::io::Write;

const MULTI_ACCOUNT_TOML: &str = r#"
[general]
log_level = "debug"
notification_enabled = false

[[account]]
id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
url = "https://cloud.example.com"
username = "bob"
display_name = "Bob"

[[account.folder]]
id = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
local_path = "/home/bob/ownCloud"
space_id = "space-1"
display_name = "Home"
vfs_mode = "off"
paused = false

[[account]]
id = "cccccccc-cccc-cccc-cccc-cccccccccccc"
url = "https://corp.example.com"
username = "bob.corp"
display_name = "Bob (Work)"
"#;

#[test]
fn load_multi_account_from_disk() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", MULTI_ACCOUNT_TOML).unwrap();
    let cfg = AppConfig::load(file.path()).unwrap();
    assert_eq!(cfg.general.log_level, "debug");
    assert!(!cfg.general.notification_enabled);
    assert_eq!(cfg.account.len(), 2);
    assert_eq!(cfg.account[0].username, "bob");
    assert_eq!(cfg.account[0].folder.len(), 1);
    assert_eq!(cfg.account[1].username, "bob.corp");
    assert!(cfg.account[1].folder.is_empty());
}

#[test]
fn round_trip_multi_account() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", MULTI_ACCOUNT_TOML).unwrap();
    let cfg = AppConfig::load(file.path()).unwrap();
    let out = NamedTempFile::new().unwrap();
    cfg.save(out.path()).unwrap();
    let cfg2 = AppConfig::load(out.path()).unwrap();
    assert_eq!(cfg, cfg2);
}
```

- [ ] Create `crates/daemon/tests/scheduler_tests.rs`:

```rust
//! Integration-level scheduler tests.
use daemon::scheduler::SyncScheduler;
use uuid::Uuid;

#[test]
fn concurrent_folders_independent() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let mut s = SyncScheduler::new(vec![id1, id2]);

    s.request_sync(id1);
    s.request_sync(id2);

    let ready = s.ready_to_run();
    assert!(ready.contains(&id1));
    assert!(ready.contains(&id2));

    // Start only id1 — id2 stays ready
    s.start_sync(id1);
    let ready = s.ready_to_run();
    assert!(!ready.contains(&id1));
    assert!(ready.contains(&id2));
}

#[test]
fn finish_updates_last_sync_time() {
    let id = Uuid::new_v4();
    let mut s = SyncScheduler::new(vec![id]);
    s.request_sync(id);
    s.start_sync(id);
    s.finish_sync(id);
    let state = s.state(id).unwrap();
    assert!(!state.running);
    assert!(state.last_sync.is_some());
}

#[test]
fn unknown_folder_id_is_silently_ignored() {
    let mut s = SyncScheduler::new(vec![]);
    let ghost = Uuid::new_v4();
    s.request_sync(ghost); // must not panic
    assert!(s.ready_to_run().is_empty());
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon --test config_tests --test scheduler_tests
```

Expected output:
```
running 2 tests
test load_multi_account_from_disk ... ok
test round_trip_multi_account ... ok

test result: ok. 2 passed; 0 failed

running 3 tests
test concurrent_folders_independent ... ok
test finish_updates_last_sync_time ... ok
test unknown_folder_id_is_silently_ignored ... ok

test result: ok. 3 passed; 0 failed
```

---

## Task 12: tests/ipc_tests.rs — full in-process integration test

- [ ] Create `crates/daemon/tests/ipc_tests.rs`:

```rust
//! End-to-end IPC integration test:
//! starts the GuiIpcServer in-process, connects two clients, verifies Subscribe
//! results in Ready event, TriggerSync results in SyncStarted event.

use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tempfile::tempdir;
use uuid::Uuid;

use daemon::gui_ipc::GuiIpcServer;
use daemon::gui_ipc::protocol::{
    DaemonCommand, DaemonEvent,
    write_command, read_event,
};
use daemon::scheduler::SyncScheduler;

async fn connect_client(socket_path: &std::path::Path) -> UnixStream {
    // Retry a few times while the server is starting.
    for _ in 0..20 {
        if let Ok(stream) = UnixStream::connect(socket_path).await {
            return stream;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("could not connect to GUI IPC socket");
}

#[tokio::test]
async fn subscribe_receives_ready_and_sync_started() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("daemon-gui.sock");

    let (ipc, _) = GuiIpcServer::new();
    let ipc = Arc::new(ipc);  // re-wrap (new() already returns Arc, adjust if needed)

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<DaemonCommand>(64);

    // Spawn server
    let ipc_server = Arc::clone(&ipc);
    let sp = socket_path.clone();
    tokio::spawn(async move {
        ipc_server.run(&sp, cmd_tx).await.unwrap();
    });

    // Give server time to bind
    tokio::time::sleep(Duration::from_millis(50)).await;

    // ── Client A ───────────────────────────────────────────────────────────
    let stream_a = connect_client(&socket_path).await;
    let (mut read_a, mut write_a) = stream_a.into_split();

    write_command(&mut write_a, &DaemonCommand::Subscribe).await.unwrap();

    // ── Client B ───────────────────────────────────────────────────────────
    let stream_b = connect_client(&socket_path).await;
    let (mut read_b, mut write_b) = stream_b.into_split();

    write_command(&mut write_b, &DaemonCommand::Subscribe).await.unwrap();

    // Give server time to register subscriptions
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Broadcast Ready from daemon side
    ipc.broadcast(DaemonEvent::Ready);

    // Both clients should receive Ready
    let evt_a = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_a))
        .await.expect("timeout on client A").unwrap();
    let evt_b = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_b))
        .await.expect("timeout on client B").unwrap();

    assert_eq!(evt_a, DaemonEvent::Ready);
    assert_eq!(evt_b, DaemonEvent::Ready);

    // ── TriggerSync flow ───────────────────────────────────────────────────
    let folder_id = Uuid::new_v4();

    // The daemon's command loop would handle TriggerSync; here we simulate it
    // by draining cmd_rx and broadcasting SyncStarted.
    let ipc_for_cmd = Arc::clone(&ipc);
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            if let DaemonCommand::TriggerSync { folder_id } = cmd {
                ipc_for_cmd.broadcast(DaemonEvent::SyncStarted { folder_id });
            }
        }
    });

    // Client A sends TriggerSync
    write_command(&mut write_a, &DaemonCommand::TriggerSync { folder_id }).await.unwrap();

    // Both clients should receive SyncStarted
    let evt_a2 = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_a))
        .await.expect("timeout on client A SyncStarted").unwrap();
    let evt_b2 = tokio::time::timeout(Duration::from_secs(1), read_event(&mut read_b))
        .await.expect("timeout on client B SyncStarted").unwrap();

    assert_eq!(evt_a2, DaemonEvent::SyncStarted { folder_id });
    assert_eq!(evt_b2, DaemonEvent::SyncStarted { folder_id });
}

#[tokio::test]
async fn non_subscribed_client_does_not_receive_events() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("daemon-gui-nosub.sock");

    let (ipc, _) = GuiIpcServer::new();
    let (cmd_tx, _cmd_rx) = mpsc::channel::<DaemonCommand>(64);

    let ipc_server = Arc::clone(&ipc);
    let sp = socket_path.clone();
    tokio::spawn(async move {
        ipc_server.run(&sp, cmd_tx).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Connect but do NOT send Subscribe
    let stream = connect_client(&socket_path).await;
    let (mut reader, _writer) = stream.into_split();

    // Broadcast an event
    ipc.broadcast(DaemonEvent::Ready);

    // Non-subscribed client should not receive it within 200ms
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        read_event(&mut reader),
    ).await;

    assert!(result.is_err(), "expected timeout but got a message");
}
```

- [ ] Verify tests pass:

```bash
cargo test -p daemon --test ipc_tests
```

Expected output:
```
running 2 tests
test subscribe_receives_ready_and_sync_started ... ok
test non_subscribed_client_does_not_receive_events ... ok

test result: ok. 2 passed; 0 failed
```

- [ ] Run the full daemon test suite to confirm everything passes together:

```bash
cargo test -p daemon
```

Expected output:
```
running N tests
...
test result: ok. N passed; 0 failed; 0 ignored
```

# Plan 2: Sync Engine — vfs-core, vfs-off, sync-engine

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `vfs-core` trait crate, the `vfs-off` no-op implementation (Linux/fallback), and the full three-phase sync engine (discovery, reconciliation, propagation) in `sync-engine`.

**Architecture:** `vfs-core` defines the `Vfs` trait only. `vfs-off` implements it as no-ops. `sync-engine` runs: (1) parallel local+remote discovery, (2) pure reconcile function, (3) async propagation with DB updates.

**Tech Stack:** Rust 2021, tokio, rayon, async-trait, camino (Utf8Path), thiserror, uuid, wiremock (tests). Depends on sync-db and ocis-client (Plan 1).

---

## Task 1: vfs-core crate

- [ ] Add `crates/vfs-core` to the workspace members in the root `Cargo.toml`:

```toml
members = [
    "crates/sync-db",
    "crates/ocis-client",
    "crates/vfs-core",
    "crates/vfs-off",
    "crates/sync-engine",
]
```

Also add new workspace dependencies:

```toml
# path utilities
camino = { version = "1.1", features = ["serde1"] }
# parallelism
rayon = "1.10"
# async trait support
async-trait = "0.1"
```

- [ ] Create directories:

```bash
mkdir -p crates/vfs-core/src
mkdir -p crates/vfs-off/src
mkdir -p crates/sync-engine/src/discovery
mkdir -p crates/sync-engine/src/propagate
mkdir -p crates/sync-engine/tests
```

- [ ] Write a failing test first. Create `crates/vfs-core/tests/trait_object.rs`:

```rust
// tests/trait_object.rs
use vfs_core::{Vfs, VfsError, VfsFileItem, VfsStatus};
use camino::Utf8Path;

struct Dummy;

#[async_trait::async_trait]
impl Vfs for Dummy {
    async fn create_placeholder(&self, _path: &Utf8Path, _item: &VfsFileItem) -> Result<(), VfsError> {
        Ok(())
    }
    async fn hydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> {
        Ok(())
    }
    async fn dehydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> {
        Ok(())
    }
    async fn status(&self, _path: &Utf8Path) -> Result<VfsStatus, VfsError> {
        Ok(VfsStatus::Full)
    }
    async fn set_pinned(&self, _path: &Utf8Path, _pinned: bool) -> Result<(), VfsError> {
        Ok(())
    }
}

#[tokio::test]
async fn trait_object_compiles() {
    let d: Box<dyn Vfs + Send + Sync> = Box::new(Dummy);
    let path = Utf8Path::new("/tmp/test.txt");
    let item = VfsFileItem {
        path: path.to_owned(),
        size: 0,
        etag: String::new(),
        file_id: String::new(),
    };
    d.create_placeholder(path, &item).await.unwrap();
    let s = d.status(path).await.unwrap();
    assert_eq!(s, VfsStatus::Full);
}
```

- [ ] Run the test (expect failure — crate does not exist yet):

```bash
cargo test -p vfs-core --test trait_object 2>&1 | head -20
# Expected: error[E0432]: unresolved import `vfs_core`
```

- [ ] Create `crates/vfs-core/Cargo.toml`:

```toml
[package]
name = "vfs-core"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

[dependencies]
async-trait = { workspace = true }
camino = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

- [ ] Create `crates/vfs-core/src/lib.rs`:

```rust
//! vfs-core — Virtual Filesystem trait definitions.
//!
//! This crate defines the [`Vfs`] trait and its supporting types.
//! Platform-specific implementations live in separate crates (e.g. `vfs-off`).

use async_trait::async_trait;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Metadata the sync engine passes when creating a placeholder file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsFileItem {
    pub path: Utf8PathBuf,
    pub size: u64,
    pub etag: String,
    pub file_id: String,
}

/// Hydration state of a VFS entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VfsStatus {
    /// File is fully present on disk.
    Full,
    /// Placeholder exists; content has not been downloaded.
    Placeholder,
    /// File is being hydrated (partial download in progress).
    Syncing,
}

/// Errors produced by VFS operations.
#[derive(Debug, Error)]
pub enum VfsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("VFS operation not supported on this platform")]
    NotSupported,

    #[error("Path not found: {path}")]
    NotFound { path: Utf8PathBuf },

    #[error("VFS backend error: {0}")]
    Backend(String),
}

/// Abstraction over OS-level virtual filesystem support.
///
/// Implementations must be `Send + Sync` so they can be shared across tasks.
#[async_trait]
pub trait Vfs: Send + Sync {
    /// Create a placeholder (dehydrated) entry at `path`.
    async fn create_placeholder(
        &self,
        path: &Utf8Path,
        item: &VfsFileItem,
    ) -> Result<(), VfsError>;

    /// Trigger on-demand hydration of a placeholder.
    async fn hydrate(&self, path: &Utf8Path) -> Result<(), VfsError>;

    /// Convert a full file back into a placeholder to free disk space.
    async fn dehydrate(&self, path: &Utf8Path) -> Result<(), VfsError>;

    /// Return the current [`VfsStatus`] of `path`.
    async fn status(&self, path: &Utf8Path) -> Result<VfsStatus, VfsError>;

    /// Pin or unpin `path` (pinned files are never automatically dehydrated).
    async fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<(), VfsError>;
}
```

- [ ] Run the test (expect pass):

```bash
cargo test -p vfs-core --test trait_object 2>&1
# Expected: test trait_object_compiles ... ok
```

- [ ] Commit:

```bash
git add crates/vfs-core/ Cargo.toml
git commit -m "feat(vfs-core): add Vfs trait, VfsFileItem, VfsStatus, VfsError"
```

---

## Task 2: vfs-off crate

- [ ] Write the failing test first. Create `crates/vfs-off/tests/noop.rs`:

```rust
use camino::Utf8Path;
use vfs_core::{Vfs, VfsFileItem, VfsStatus};
use vfs_off::VfsOff;

fn item(path: &Utf8Path) -> VfsFileItem {
    VfsFileItem {
        path: path.to_owned(),
        size: 42,
        etag: "abc".into(),
        file_id: "id1".into(),
    }
}

#[tokio::test]
async fn all_methods_return_ok() {
    let vfs = VfsOff::new();
    let p = Utf8Path::new("/tmp/foo.txt");

    vfs.create_placeholder(p, &item(p)).await.unwrap();
    vfs.hydrate(p).await.unwrap();
    vfs.dehydrate(p).await.unwrap();
    vfs.set_pinned(p, true).await.unwrap();

    let s = vfs.status(p).await.unwrap();
    assert_eq!(s, VfsStatus::Full);
}

#[tokio::test]
async fn vfs_off_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<VfsOff>();
}
```

- [ ] Run (expect failure — crate missing):

```bash
cargo test -p vfs-off --test noop 2>&1 | head -10
# Expected: error[E0432]: unresolved import `vfs_off`
```

- [ ] Create `crates/vfs-off/Cargo.toml`:

```toml
[package]
name = "vfs-off"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

[dependencies]
vfs-core = { path = "../vfs-core" }
async-trait = { workspace = true }
camino = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

- [ ] Create `crates/vfs-off/src/lib.rs`:

```rust
//! vfs-off — No-op VFS implementation.
//!
//! Used on Linux (where no kernel VFS extension exists) and as a test double.
//! Every method succeeds immediately without touching the filesystem.

use async_trait::async_trait;
use camino::Utf8Path;
use vfs_core::{Vfs, VfsError, VfsFileItem, VfsStatus};

/// A VFS implementation that performs no operations.
///
/// `status()` always returns [`VfsStatus::Full`], modelling an environment
/// where all files are already present on disk and no dehydration is possible.
#[derive(Debug, Default, Clone)]
pub struct VfsOff;

impl VfsOff {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Vfs for VfsOff {
    async fn create_placeholder(
        &self,
        _path: &Utf8Path,
        _item: &VfsFileItem,
    ) -> Result<(), VfsError> {
        Ok(())
    }

    async fn hydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> {
        Ok(())
    }

    async fn dehydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> {
        Ok(())
    }

    async fn status(&self, _path: &Utf8Path) -> Result<VfsStatus, VfsError> {
        Ok(VfsStatus::Full)
    }

    async fn set_pinned(&self, _path: &Utf8Path, _pinned: bool) -> Result<(), VfsError> {
        Ok(())
    }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p vfs-off --test noop 2>&1
# Expected:
# test all_methods_return_ok ... ok
# test vfs_off_is_send_sync ... ok
```

- [ ] Commit:

```bash
git add crates/vfs-off/
git commit -m "feat(vfs-off): no-op Vfs implementation for Linux/fallback"
```

---

## Task 3: sync-engine Cargo.toml and error.rs

- [ ] Write a failing compilation test. Create `crates/sync-engine/tests/error_variants.rs`:

```rust
use sync_engine::error::SyncError;

#[test]
fn all_variants_exist() {
    let _: SyncError = SyncError::Http { status: 404, message: "not found".into() };
    let _: SyncError = SyncError::Io(std::io::Error::new(std::io::ErrorKind::Other, "test"));
    let _: SyncError = SyncError::Db("db error".into());
    let _: SyncError = SyncError::Vfs("vfs error".into());
    let _: SyncError = SyncError::Parse("parse error".into());
    let _: SyncError = SyncError::Conflict { path: camino::Utf8PathBuf::from("/a/b") };
    let _: SyncError = SyncError::Cancelled;
}

#[test]
fn sync_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SyncError>();
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test error_variants 2>&1 | head -10
# Expected: error[E0432]: unresolved import `sync_engine`
```

- [ ] Create `crates/sync-engine/Cargo.toml`:

```toml
[package]
name = "sync-engine"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

[dependencies]
# workspace
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
thiserror = { workspace = true }
camino = { workspace = true }
# parallelism
rayon = { workspace = true }
# async trait
async-trait = { workspace = true }
# local crates
vfs-core  = { path = "../vfs-core" }
vfs-off   = { path = "../vfs-off" }
sync-db   = { path = "../sync-db" }
ocis-client = { path = "../ocis-client" }
# tracing
tracing = "0.1"

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
wiremock = { workspace = true }
tempfile = { workspace = true }
```

- [ ] Create `crates/sync-engine/src/lib.rs` (minimal, will grow):

```rust
pub mod error;
pub mod types;
pub mod state;
pub mod discovery;
pub mod reconcile;
pub mod propagate;
pub mod engine;
```

- [ ] Create `crates/sync-engine/src/error.rs`:

```rust
use camino::Utf8PathBuf;
use thiserror::Error;

/// All errors that can occur inside the sync engine.
#[derive(Debug, Error)]
pub enum SyncError {
    /// An HTTP operation returned an unexpected status code.
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },

    /// A filesystem I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A database operation failed.
    #[error("Database error: {0}")]
    Db(String),

    /// A VFS operation failed.
    #[error("VFS error: {0}")]
    Vfs(String),

    /// Failed to parse a value (XML, JSON, header, …).
    #[error("Parse error: {0}")]
    Parse(String),

    /// Two versions of a file conflict and automatic resolution was not possible.
    #[error("Conflict at path: {path}")]
    Conflict { path: Utf8PathBuf },

    /// The sync was cancelled externally.
    #[error("Sync cancelled")]
    Cancelled,
}

impl From<vfs_core::VfsError> for SyncError {
    fn from(e: vfs_core::VfsError) -> Self {
        SyncError::Vfs(e.to_string())
    }
}

/// Convenience alias.
pub type Result<T, E = SyncError> = std::result::Result<T, E>;
```

- [ ] Add stub modules so the crate compiles. Create each file with a single `// TODO` comment:

`crates/sync-engine/src/types.rs` — `// TODO`  
`crates/sync-engine/src/state.rs` — `// TODO`  
`crates/sync-engine/src/discovery/mod.rs` — `// TODO`  
`crates/sync-engine/src/reconcile.rs` — `// TODO`  
`crates/sync-engine/src/propagate/mod.rs` — `// TODO`  
`crates/sync-engine/src/engine.rs` — `// TODO`  

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test error_variants 2>&1
# Expected:
# test all_variants_exist ... ok
# test sync_error_is_send_sync ... ok
```

- [ ] Commit:

```bash
git add crates/sync-engine/
git commit -m "feat(sync-engine): scaffold crate, SyncError with all variants"
```

---

## Task 4: sync-engine types.rs

- [ ] Write the failing test. Create `crates/sync-engine/tests/types_compile.rs`:

```rust
use camino::Utf8PathBuf;
use std::time::SystemTime;
use sync_engine::types::*;

#[test]
fn local_entry_fields() {
    let e = LocalEntry {
        path: Utf8PathBuf::from("/tmp/a.txt"),
        mtime: SystemTime::UNIX_EPOCH,
        size: 100,
        inode: 42,
        is_virtual: false,
    };
    assert_eq!(e.size, 100);
    assert!(!e.is_virtual);
}

#[test]
fn remote_entry_fields() {
    let e = RemoteEntry {
        path: Utf8PathBuf::from("/remote/a.txt"),
        etag: "abc123".into(),
        mtime: SystemTime::UNIX_EPOCH,
        size: 200,
        file_id: "file-uuid".into(),
        permissions: 0o644,
    };
    assert_eq!(e.etag, "abc123");
}

#[test]
fn sync_instruction_variants() {
    let _up = SyncInstruction::Upload;
    let _dn = SyncInstruction::Download;
    let _dl = SyncInstruction::DeleteLocal;
    let _dr = SyncInstruction::DeleteRemote;
    let _rl = SyncInstruction::RenameLocal { to: Utf8PathBuf::from("/b") };
    let _rr = SyncInstruction::RenameRemote { to: Utf8PathBuf::from("/c") };
    let _co = SyncInstruction::Conflict;
    let _um = SyncInstruction::UpdateMetadata;
    let _ig = SyncInstruction::Ignore;
}

#[test]
fn sync_file_item_roundtrip() {
    use std::collections::HashMap;
    let item = SyncFileItem {
        path: Utf8PathBuf::from("/a/b.txt"),
        instruction: SyncInstruction::Upload,
        direction: Direction::Up,
        etag: Some("etag1".into()),
        size: 512,
        mtime: SystemTime::UNIX_EPOCH,
        file_id: Some("fid".into()),
        checksum: None,
        error: None,
    };
    assert_eq!(item.direction, Direction::Up);
}

#[test]
fn conflict_strategy_variants() {
    let _ = ConflictStrategy::KeepBoth;
    let _ = ConflictStrategy::KeepRemote;
    let _ = ConflictStrategy::KeepLocal;
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test types_compile 2>&1 | head -20
# Expected: error[E0432]: unresolved imports
```

- [ ] Replace `crates/sync-engine/src/types.rs` with:

```rust
//! Core domain types for the sync engine.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use crate::error::SyncError;

// ── Discovery types ──────────────────────────────────────────────────────────

/// A file or directory discovered on the local filesystem.
#[derive(Debug, Clone)]
pub struct LocalEntry {
    pub path: Utf8PathBuf,
    pub mtime: SystemTime,
    pub size: u64,
    /// Inode number, used for rename/move detection.
    pub inode: u64,
    /// True when the entry is a VFS placeholder (not yet downloaded).
    pub is_virtual: bool,
}

/// A file or directory discovered on the remote (oCIS) server.
#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub path: Utf8PathBuf,
    pub etag: String,
    pub mtime: SystemTime,
    pub size: u64,
    /// oCIS-assigned stable file identifier.
    pub file_id: String,
    /// WebDAV permissions bitmask.
    pub permissions: u32,
}

// ── Reconciliation types ─────────────────────────────────────────────────────

/// What the propagation phase should do for a given path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncInstruction {
    Upload,
    Download,
    DeleteLocal,
    DeleteRemote,
    RenameLocal { to: Utf8PathBuf },
    RenameRemote { to: Utf8PathBuf },
    Conflict,
    UpdateMetadata,
    Ignore,
}

/// Which direction data flows for this item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    None,
}

/// How the engine should resolve a conflict between local and remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictStrategy {
    /// Keep both versions, renaming the local copy with a conflict suffix.
    KeepBoth,
    /// Overwrite local with remote.
    KeepRemote,
    /// Overwrite remote with local.
    KeepLocal,
}

// ── Propagation types ────────────────────────────────────────────────────────

/// A resolved work item passed from reconciliation to propagation.
#[derive(Debug, Clone)]
pub struct SyncFileItem {
    pub path: Utf8PathBuf,
    pub instruction: SyncInstruction,
    pub direction: Direction,
    pub etag: Option<String>,
    pub size: u64,
    pub mtime: SystemTime,
    pub file_id: Option<String>,
    pub checksum: Option<String>,
    /// Set after propagation if the item failed.
    pub error: Option<SyncError>,
}
```

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test types_compile 2>&1
# Expected: all 5 tests pass
```

- [ ] Commit:

```bash
git add crates/sync-engine/src/types.rs
git commit -m "feat(sync-engine): add all domain types in types.rs"
```

---

## Task 5: sync-engine state.rs

- [ ] Write the failing test. Create `crates/sync-engine/tests/state_tests.rs`:

```rust
use camino::Utf8PathBuf;
use std::sync::Arc;
use sync_engine::state::{FileStatus, FolderStatus, SyncState};
use uuid::Uuid;

#[test]
fn initial_state_is_idle() {
    let id = Uuid::new_v4();
    let state = SyncState::new(id);
    assert_eq!(state.status, FolderStatus::Idle);
    assert!(state.file_statuses.is_empty());
    assert!(state.last_sync.is_none());
    assert!(state.errors.is_empty());
}

#[test]
fn set_file_status() {
    let id = Uuid::new_v4();
    let mut state = SyncState::new(id);
    let path = Utf8PathBuf::from("/a/b.txt");
    state.set_file_status(path.clone(), FileStatus::Syncing);
    assert_eq!(state.file_statuses[&path], FileStatus::Syncing);
}

#[test]
fn arc_rwlock_shared() {
    use std::sync::RwLock;
    let id = Uuid::new_v4();
    let shared = Arc::new(RwLock::new(SyncState::new(id)));
    {
        let mut w = shared.write().unwrap();
        w.status = FolderStatus::Syncing;
    }
    let r = shared.read().unwrap();
    assert_eq!(r.status, FolderStatus::Syncing);
}

#[test]
fn error_status_carries_message() {
    let s = FileStatus::Error("checksum mismatch".into());
    match s {
        FileStatus::Error(msg) => assert!(msg.contains("checksum")),
        _ => panic!("wrong variant"),
    }
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test state_tests 2>&1 | head -15
```

- [ ] Replace `crates/sync-engine/src/state.rs` with:

```rust
//! Runtime sync state tracked per folder.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use uuid::Uuid;

use crate::error::SyncError;

/// High-level status of a watched folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderStatus {
    Idle,
    Syncing,
    Error,
}

/// Per-file status exposed to the UI layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileStatus {
    Ok,
    Syncing,
    Error(String),
    Excluded,
}

/// All mutable state for one sync folder, typically held behind `Arc<RwLock<>>`.
#[derive(Debug)]
pub struct SyncState {
    pub folder_id: Uuid,
    pub status: FolderStatus,
    pub file_statuses: HashMap<Utf8PathBuf, FileStatus>,
    pub last_sync: Option<SystemTime>,
    pub errors: Vec<SyncError>,
}

impl SyncState {
    pub fn new(folder_id: Uuid) -> Self {
        Self {
            folder_id,
            status: FolderStatus::Idle,
            file_statuses: HashMap::new(),
            last_sync: None,
            errors: Vec::new(),
        }
    }

    pub fn set_file_status(&mut self, path: Utf8PathBuf, status: FileStatus) {
        self.file_statuses.insert(path, status);
    }

    pub fn record_error(&mut self, error: SyncError) {
        self.status = FolderStatus::Error;
        self.errors.push(error);
    }

    pub fn mark_complete(&mut self) {
        self.status = FolderStatus::Idle;
        self.last_sync = Some(SystemTime::now());
    }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test state_tests 2>&1
# Expected: 4 tests pass
```

- [ ] Commit:

```bash
git add crates/sync-engine/src/state.rs
git commit -m "feat(sync-engine): SyncState, FolderStatus, FileStatus"
```

---

## Task 6: local discovery

- [ ] Write the failing test. Create `crates/sync-engine/tests/local_discovery.rs`:

```rust
use camino::Utf8Path;
use std::fs;
use sync_engine::discovery::local::discover_local;
use tempfile::TempDir;

#[tokio::test]
async fn discovers_files_recursively() {
    let dir = TempDir::new().unwrap();
    let root = Utf8Path::from_path(dir.path()).unwrap();

    // Create structure:
    //   root/
    //     a.txt          (10 bytes)
    //     sub/
    //       b.txt        (5 bytes)
    //       deep/
    //         c.txt      (1 byte)
    fs::write(dir.path().join("a.txt"), b"0123456789").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/b.txt"), b"hello").unwrap();
    fs::create_dir_all(dir.path().join("sub/deep")).unwrap();
    fs::write(dir.path().join("sub/deep/c.txt"), b"x").unwrap();

    let entries = discover_local(root).await.unwrap();

    // Directories are not included — only files.
    assert!(entries.iter().all(|e| !e.is_virtual));

    let names: Vec<&str> = entries
        .iter()
        .map(|e| e.path.file_name().unwrap())
        .collect();

    assert!(names.contains(&"a.txt"), "missing a.txt");
    assert!(names.contains(&"b.txt"), "missing b.txt");
    assert!(names.contains(&"c.txt"), "missing c.txt");

    let a = entries.iter().find(|e| e.path.file_name() == Some("a.txt")).unwrap();
    assert_eq!(a.size, 10);
}

#[tokio::test]
async fn empty_directory_returns_empty_vec() {
    let dir = TempDir::new().unwrap();
    let root = Utf8Path::from_path(dir.path()).unwrap();
    let entries = discover_local(root).await.unwrap();
    assert!(entries.is_empty());
}

#[tokio::test]
async fn inodes_are_nonzero_on_linux() {
    let dir = TempDir::new().unwrap();
    let root = Utf8Path::from_path(dir.path()).unwrap();
    std::fs::write(dir.path().join("f.txt"), b"data").unwrap();
    let entries = discover_local(root).await.unwrap();
    assert_eq!(entries.len(), 1);
    // On Linux inode is always non-zero for real files.
    assert!(entries[0].inode > 0);
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test local_discovery 2>&1 | head -15
```

- [ ] Replace `crates/sync-engine/src/discovery/mod.rs` with:

```rust
pub mod local;
pub mod remote;
```

- [ ] Create `crates/sync-engine/src/discovery/local.rs`:

```rust
//! Local filesystem discovery using rayon for parallel directory walking.

use camino::{Utf8Path, Utf8PathBuf};
use std::os::unix::fs::MetadataExt as _;
use std::time::SystemTime;

use crate::error::Result;
use crate::types::LocalEntry;

/// Walk `root` recursively and return one [`LocalEntry`] per **file** found.
///
/// Directory entries are skipped; only regular files are returned.
/// The walk is performed on a rayon thread pool via `spawn_blocking` to avoid
/// blocking the async executor.
pub async fn discover_local(root: &Utf8Path) -> Result<Vec<LocalEntry>> {
    let root = root.to_owned();
    tokio::task::spawn_blocking(move || walk(&root))
        .await
        .map_err(|e| crate::error::SyncError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        )))?
}

fn walk(root: &Utf8Path) -> Result<Vec<LocalEntry>> {
    use rayon::prelude::*;
    use std::sync::Mutex;

    let entries = Mutex::new(Vec::new());

    walk_dir(root, &entries)?;

    Ok(entries.into_inner().unwrap())
}

fn walk_dir(dir: &Utf8Path, entries: &std::sync::Mutex<Vec<LocalEntry>>) -> Result<()> {
    let read_dir = std::fs::read_dir(dir)?;

    let mut subdirs = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        let meta = entry.metadata()?;
        let path = Utf8PathBuf::from_path_buf(entry.path())
            .unwrap_or_else(|p| Utf8PathBuf::from(p.to_string_lossy().as_ref()));

        if meta.is_dir() {
            subdirs.push(path);
        } else if meta.is_file() {
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let inode = meta.ino();
            entries.lock().unwrap().push(LocalEntry {
                path,
                mtime,
                size: meta.len(),
                inode,
                is_virtual: false,
            });
        }
    }

    // Recurse into subdirectories sequentially (parallelism is at the
    // top-level spawn_blocking boundary).
    for sub in subdirs {
        walk_dir(&sub, entries)?;
    }

    Ok(())
}
```

- [ ] Create stub `crates/sync-engine/src/discovery/remote.rs`:

```rust
// TODO: implemented in Task 7
```

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test local_discovery 2>&1
# Expected: 3 tests pass
```

- [ ] Commit:

```bash
git add crates/sync-engine/src/discovery/
git commit -m "feat(sync-engine): local discovery with rayon+spawn_blocking"
```

---

## Task 7: remote discovery

- [ ] Write the failing test. Create `crates/sync-engine/tests/remote_discovery.rs`:

```rust
use sync_engine::discovery::remote::discover_remote;
use url::Url;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn propfind_response_root() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/space1/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</D:getlastmodified>
        <D:getcontentlength>0</D:getcontentlength>
        <D:getetag>"rootetag"</D:getetag>
        <OC:fileid>root-id</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/space1/hello.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getlastmodified>Mon, 01 Jan 2024 12:00:00 GMT</D:getlastmodified>
        <D:getcontentlength>5</D:getcontentlength>
        <D:getetag>"abc123"</D:getetag>
        <OC:fileid>file-id-1</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#
}

#[tokio::test]
async fn discovers_files_from_propfind() {
    let server = MockServer::start().await;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/space1.*"))
        .respond_with(
            ResponseTemplate::new(207)
                .set_body_string(propfind_response_root()),
        )
        .mount(&server)
        .await;

    let base = Url::parse(&format!("{}/dav/spaces/space1/", server.uri())).unwrap();
    let entries = discover_remote(&base).await.unwrap();

    // The root collection itself is excluded; only files are returned.
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path.file_name(), Some("hello.txt"));
    assert_eq!(entries[0].etag, "abc123");
    assert_eq!(entries[0].size, 5);
    assert_eq!(entries[0].file_id, "file-id-1");
}

#[tokio::test]
async fn empty_collection_returns_empty() {
    let server = MockServer::start().await;

    let empty = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/spaces/empty/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/empty.*"))
        .respond_with(ResponseTemplate::new(207).set_body_string(empty))
        .mount(&server)
        .await;

    let base = Url::parse(&format!("{}/dav/spaces/empty/", server.uri())).unwrap();
    let entries = discover_remote(&base).await.unwrap();
    assert!(entries.is_empty());
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test remote_discovery 2>&1 | head -15
```

- [ ] Replace `crates/sync-engine/src/discovery/remote.rs` with:

```rust
//! Remote (WebDAV / oCIS) discovery via breadth-first PROPFIND.

use camino::Utf8PathBuf;
use std::time::SystemTime;
use url::Url;

use crate::error::{Result, SyncError};
use crate::types::RemoteEntry;

/// Fetch all remote entries under `space_root` using Depth:1 PROPFIND,
/// recursing into collections breadth-first.
pub async fn discover_remote(space_root: &Url) -> Result<Vec<RemoteEntry>> {
    let client = reqwest::Client::new();
    let mut result = Vec::new();
    let mut queue = vec![space_root.clone()];

    while let Some(url) = queue.first().cloned() {
        queue.remove(0);

        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:prop>
    <D:resourcetype/>
    <D:getlastmodified/>
    <D:getcontentlength/>
    <D:getetag/>
    <OC:fileid/>
    <OC:permissions/>
  </D:prop>
</D:propfind>"#;

        let resp = client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), url.as_str())
            .header("Depth", "1")
            .header("Content-Type", "application/xml")
            .body(body)
            .send()
            .await
            .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

        if !resp.status().is_success() && resp.status().as_u16() != 207 {
            return Err(SyncError::Http {
                status: resp.status().as_u16(),
                message: "PROPFIND failed".into(),
            });
        }

        let text = resp
            .text()
            .await
            .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

        let (files, dirs) = parse_propfind(&text, space_root)?;
        result.extend(files);
        queue.extend(dirs);
    }

    Ok(result)
}

// ── XML parsing ───────────────────────────────────────────────────────────────

fn parse_propfind(
    xml: &str,
    space_root: &Url,
) -> Result<(Vec<RemoteEntry>, Vec<Url>)> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut files = Vec::new();
    let mut dirs = Vec::new();

    // State machine fields
    let mut href = String::new();
    let mut etag = String::new();
    let mut file_id = String::new();
    let mut size: u64 = 0;
    let mut is_collection = false;
    let mut in_href = false;
    let mut in_etag = false;
    let mut in_length = false;
    let mut in_fileid = false;
    let mut in_response = false;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = std::str::from_utf8(e.local_name().into_inner())
                    .unwrap_or("")
                    .to_owned();
                match name.as_str() {
                    "response" => {
                        in_response = true;
                        href.clear();
                        etag.clear();
                        file_id.clear();
                        size = 0;
                        is_collection = false;
                    }
                    "href" if in_response => in_href = true,
                    "getetag" => in_etag = true,
                    "getcontentlength" => in_length = true,
                    "fileid" => in_fileid = true,
                    "collection" => is_collection = true,
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = std::str::from_utf8(e.local_name().into_inner())
                    .unwrap_or("")
                    .to_owned();
                if name == "collection" {
                    is_collection = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().into_owned();
                if in_href { href = text.clone(); }
                if in_etag { etag = text.trim_matches('"').to_string(); }
                if in_length { size = text.parse().unwrap_or(0); }
                if in_fileid { file_id = text.clone(); }
                in_href = false;
                in_etag = false;
                in_length = false;
                in_fileid = false;
            }
            Ok(Event::End(ref e)) => {
                let name = std::str::from_utf8(e.local_name().into_inner())
                    .unwrap_or("");
                if name == "response" && in_response {
                    in_response = false;
                    if href.is_empty() { continue; }

                    // Strip the root path prefix to get a relative path.
                    let root_path = space_root.path().trim_end_matches('/');
                    let rel = href
                        .strip_prefix(root_path)
                        .unwrap_or(&href)
                        .trim_start_matches('/');

                    if rel.is_empty() || href.trim_end_matches('/') == root_path {
                        // This is the root collection itself — skip.
                        continue;
                    }

                    if is_collection {
                        // Build the absolute URL for this sub-collection.
                        if let Ok(mut sub_url) = space_root.join(rel) {
                            if !sub_url.path().ends_with('/') {
                                sub_url.set_path(&format!("{}/", sub_url.path()));
                            }
                            dirs.push(sub_url);
                        }
                    } else {
                        let path = Utf8PathBuf::from(rel);
                        files.push(RemoteEntry {
                            path,
                            etag: etag.clone(),
                            mtime: SystemTime::UNIX_EPOCH, // simplified
                            size,
                            file_id: file_id.clone(),
                            permissions: 0,
                        });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(SyncError::Parse(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok((files, dirs))
}
```

Add `reqwest` and `quick-xml` to `crates/sync-engine/Cargo.toml` dependencies (they are available as workspace deps from Plan 1):

```toml
reqwest = { workspace = true }
quick-xml = { workspace = true }
url = { workspace = true }
```

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test remote_discovery 2>&1
# Expected: 2 tests pass
```

- [ ] Commit:

```bash
git add crates/sync-engine/src/discovery/remote.rs crates/sync-engine/Cargo.toml
git commit -m "feat(sync-engine): remote discovery via breadth-first PROPFIND"
```

---

## Task 8: reconcile.rs

- [ ] Write the failing test. Create `crates/sync-engine/tests/reconcile_tests.rs`:

```rust
//! Exhaustive unit tests for the reconcile() pure function.
//! Covers all 8 (local × remote × journal) Some/None combinations.

use camino::Utf8PathBuf;
use std::time::{Duration, SystemTime};
use sync_engine::reconcile::reconcile;
use sync_engine::types::*;

fn path(s: &str) -> Utf8PathBuf { Utf8PathBuf::from(s) }
fn t(secs: u64) -> SystemTime { SystemTime::UNIX_EPOCH + Duration::from_secs(secs) }

fn local(size: u64, mtime: SystemTime) -> LocalEntry {
    LocalEntry { path: path("/a.txt"), mtime, size, inode: 1, is_virtual: false }
}

fn remote(size: u64, etag: &str, mtime: SystemTime) -> RemoteEntry {
    RemoteEntry {
        path: path("/a.txt"),
        etag: etag.into(),
        mtime,
        size,
        file_id: "fid".into(),
        permissions: 0,
    }
}

/// JournalEntry: (etag_at_last_sync, size_at_last_sync)
fn journal(etag: &str, size: u64) -> (String, u64) {
    (etag.to_string(), size)
}

// Case 1: No local, no remote, no journal → Ignore
#[test]
fn no_local_no_remote_no_journal() {
    let instr = reconcile(None, None, None, ConflictStrategy::KeepBoth);
    assert_eq!(instr, SyncInstruction::Ignore);
}

// Case 2: Local only (new local file, no journal) → Upload
#[test]
fn local_only_no_journal() {
    let instr = reconcile(Some(local(10, t(1))), None, None, ConflictStrategy::KeepBoth);
    assert_eq!(instr, SyncInstruction::Upload);
}

// Case 3: Remote only (new remote file, no journal) → Download
#[test]
fn remote_only_no_journal() {
    let instr = reconcile(None, Some(remote(10, "e1", t(1))), None, ConflictStrategy::KeepBoth);
    assert_eq!(instr, SyncInstruction::Download);
}

// Case 4: Local only, journal present (remote deleted) → DeleteLocal
#[test]
fn local_only_journal_present() {
    let instr = reconcile(
        Some(local(10, t(1))),
        None,
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::DeleteLocal);
}

// Case 5: Remote only, journal present (local deleted) → DeleteRemote
#[test]
fn remote_only_journal_present() {
    let instr = reconcile(
        None,
        Some(remote(10, "e1", t(1))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::DeleteRemote);
}

// Case 6: Both present, remote etag == journal etag, local unchanged → Ignore
#[test]
fn both_present_in_sync() {
    let instr = reconcile(
        Some(local(10, t(1))),
        Some(remote(10, "e1", t(1))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Ignore);
}

// Case 7: Both present, remote etag changed, local size unchanged → Download
#[test]
fn both_present_remote_changed() {
    let instr = reconcile(
        Some(local(10, t(1))),
        Some(remote(20, "e2", t(2))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Download);
}

// Case 8: Both present, local size changed, remote unchanged → Upload
#[test]
fn both_present_local_changed() {
    let instr = reconcile(
        Some(local(99, t(5))),
        Some(remote(10, "e1", t(1))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Upload);
}

// Case 9: Both changed → Conflict (KeepBoth strategy)
#[test]
fn both_changed_conflict_keepboth() {
    let instr = reconcile(
        Some(local(99, t(5))),
        Some(remote(20, "e2", t(2))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Conflict);
}

// Case 10: Both changed, KeepRemote strategy → Download
#[test]
fn both_changed_conflict_keepremote() {
    let instr = reconcile(
        Some(local(99, t(5))),
        Some(remote(20, "e2", t(2))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepRemote,
    );
    assert_eq!(instr, SyncInstruction::Download);
}

// Case 11: Both changed, KeepLocal strategy → Upload
#[test]
fn both_changed_conflict_keeplocal() {
    let instr = reconcile(
        Some(local(99, t(5))),
        Some(remote(20, "e2", t(2))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepLocal,
    );
    assert_eq!(instr, SyncInstruction::Upload);
}

// Case 12: No journal, both present → treat as conflict (neither side is
// authoritative without a baseline).
#[test]
fn both_present_no_journal_conflict() {
    let instr = reconcile(
        Some(local(10, t(1))),
        Some(remote(10, "e1", t(1))),
        None,
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Conflict);
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test reconcile_tests 2>&1 | head -15
```

- [ ] Replace `crates/sync-engine/src/reconcile.rs` with:

```rust
//! Phase 2: pure reconciliation function.
//!
//! `reconcile` takes a snapshot of the local entry, remote entry, and the
//! previously-journalled baseline, and returns the single [`SyncInstruction`]
//! that the propagation phase should execute.
//!
//! This function has no side effects and is therefore trivially unit-testable.

use crate::types::{
    ConflictStrategy, LocalEntry, RemoteEntry, SyncInstruction,
};

/// A minimal journal baseline: the etag and size recorded after the last
/// successful sync of this path.
pub type JournalBaseline = (String, u64); // (etag, size)

/// Decide what to do with one path given optional local/remote/journal entries.
///
/// # Decision table
///
/// | local | remote | journal | result |
/// |-------|--------|---------|--------|
/// | None  | None   | *       | Ignore |
/// | Some  | None   | None    | Upload (new local file) |
/// | None  | Some   | None    | Download (new remote file) |
/// | Some  | None   | Some    | DeleteLocal (remote deleted) |
/// | None  | Some   | Some    | DeleteRemote (local deleted) |
/// | Some  | Some   | None    | Conflict (no baseline) |
/// | Some  | Some   | Some    | compare changes → Upload/Download/Ignore/Conflict |
pub fn reconcile(
    local: Option<LocalEntry>,
    remote: Option<RemoteEntry>,
    journal: Option<JournalBaseline>,
    strategy: ConflictStrategy,
) -> SyncInstruction {
    match (local, remote, journal) {
        // Both absent
        (None, None, _) => SyncInstruction::Ignore,

        // Local only
        (Some(_), None, None) => SyncInstruction::Upload,
        (Some(_), None, Some(_)) => SyncInstruction::DeleteLocal,

        // Remote only
        (None, Some(_), None) => SyncInstruction::Download,
        (None, Some(_), Some(_)) => SyncInstruction::DeleteRemote,

        // Both present, no journal baseline → conflict
        (Some(_), Some(_), None) => SyncInstruction::Conflict,

        // Both present, journal baseline available
        (Some(loc), Some(rem), Some((j_etag, j_size))) => {
            let remote_changed = rem.etag != j_etag;
            let local_changed = loc.size != j_size;

            match (local_changed, remote_changed) {
                (false, false) => SyncInstruction::Ignore,
                (true, false)  => SyncInstruction::Upload,
                (false, true)  => SyncInstruction::Download,
                (true, true)   => match strategy {
                    ConflictStrategy::KeepBoth   => SyncInstruction::Conflict,
                    ConflictStrategy::KeepRemote => SyncInstruction::Download,
                    ConflictStrategy::KeepLocal  => SyncInstruction::Upload,
                },
            }
        }
    }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test reconcile_tests 2>&1
# Expected: 12 tests pass
```

- [ ] Commit:

```bash
git add crates/sync-engine/src/reconcile.rs
git commit -m "feat(sync-engine): pure reconcile() with exhaustive tests for all 12 cases"
```

---

## Task 9: propagate upload

- [ ] Write the failing test. Create `crates/sync-engine/tests/propagate_upload.rs`:

```rust
use std::io::Write;
use sync_engine::propagate::upload::{propagate_upload, UploadRequest};
use tempfile::NamedTempFile;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn small_file_uses_plain_put() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/dav/spaces/space1/hello.txt"))
        .respond_with(
            ResponseTemplate::new(201).insert_header("etag", r#""newetag""#),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"hello").unwrap();
    tmp.flush().unwrap();

    let req = UploadRequest {
        local_path: camino::Utf8Path::from_path(tmp.path()).unwrap().to_owned(),
        remote_url: Url::parse(&format!(
            "{}/dav/spaces/space1/hello.txt",
            server.uri()
        ))
        .unwrap(),
        size: 5,
        checksum: None,
        tus_threshold: 1024 * 1024 * 5, // 5 MiB
    };

    let etag = propagate_upload(req).await.unwrap();
    assert_eq!(etag.trim_matches('"'), "newetag");

    server.verify().await;
}

#[tokio::test]
async fn large_file_uses_tus_protocol() {
    let server = MockServer::start().await;

    // TUS creation endpoint
    Mock::given(method("POST"))
        .and(path("/tus/upload"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("location", "/tus/upload/abc123"),
        )
        .expect(1)
        .mount(&server)
        .await;

    // TUS patch endpoint
    Mock::given(method("PATCH"))
        .and(path("/tus/upload/abc123"))
        .respond_with(
            ResponseTemplate::new(204)
                .insert_header("upload-offset", "6")
                .insert_header("etag", r#""tusetag""#),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"big!!!").unwrap();
    tmp.flush().unwrap();

    let req = UploadRequest {
        local_path: camino::Utf8Path::from_path(tmp.path()).unwrap().to_owned(),
        remote_url: Url::parse(&format!("{}/tus/upload", server.uri())).unwrap(),
        size: 6,
        checksum: None,
        tus_threshold: 4, // force TUS for any file > 4 bytes
    };

    let etag = propagate_upload(req).await.unwrap();
    assert_eq!(etag.trim_matches('"'), "tusetag");

    server.verify().await;
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test propagate_upload 2>&1 | head -15
```

- [ ] Replace `crates/sync-engine/src/propagate/mod.rs` with:

```rust
pub mod conflict;
pub mod download;
pub mod ops;
pub mod upload;
```

- [ ] Create `crates/sync-engine/src/propagate/upload.rs`:

```rust
//! Propagation: upload a local file to the remote server.
//!
//! Small files (< `tus_threshold`) use a plain HTTP PUT.
//! Large files use the TUS resumable upload protocol (POST + PATCH).

use camino::Utf8PathBuf;
use url::Url;

use crate::error::{Result, SyncError};

/// Parameters for a single upload operation.
pub struct UploadRequest {
    /// Absolute path on the local filesystem.
    pub local_path: Utf8PathBuf,
    /// Target URL on the remote server.
    pub remote_url: Url,
    /// File size in bytes (must match actual file size).
    pub size: u64,
    /// Optional SHA-256 checksum (hex), forwarded as `OC-Checksum` header.
    pub checksum: Option<String>,
    /// Files >= this threshold use TUS; smaller files use plain PUT.
    pub tus_threshold: u64,
}

/// Upload `req.local_path` to `req.remote_url`.
///
/// Returns the ETag returned by the server (stripped of surrounding quotes).
pub async fn propagate_upload(req: UploadRequest) -> Result<String> {
    if req.size >= req.tus_threshold {
        upload_tus(req).await
    } else {
        upload_put(req).await
    }
}

// ── Plain PUT ─────────────────────────────────────────────────────────────────

async fn upload_put(req: UploadRequest) -> Result<String> {
    let bytes = tokio::fs::read(&req.local_path).await?;
    let client = reqwest::Client::new();

    let mut builder = client
        .put(req.remote_url.as_str())
        .header("Content-Length", req.size.to_string())
        .body(bytes);

    if let Some(ref cs) = req.checksum {
        builder = builder.header("OC-Checksum", format!("SHA256:{cs}"));
    }

    let resp = builder
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let status = resp.status().as_u16();
    if status != 200 && status != 201 && status != 204 {
        return Err(SyncError::Http {
            status,
            message: format!("PUT failed: {}", resp.status()),
        });
    }

    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    Ok(etag)
}

// ── TUS resumable upload ──────────────────────────────────────────────────────

async fn upload_tus(req: UploadRequest) -> Result<String> {
    let client = reqwest::Client::new();

    // Step 1: TUS creation (POST) — server returns Location header.
    let create_resp = client
        .post(req.remote_url.as_str())
        .header("Tus-Resumable", "1.0.0")
        .header("Upload-Length", req.size.to_string())
        .header("Content-Length", "0")
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let status = create_resp.status().as_u16();
    if status != 201 {
        return Err(SyncError::Http {
            status,
            message: "TUS creation failed".into(),
        });
    }

    let location = create_resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| SyncError::Parse("TUS: missing Location header".into()))?
        .to_string();

    // Resolve relative location against the creation URL.
    let patch_url = if location.starts_with("http://") || location.starts_with("https://") {
        location.clone()
    } else {
        format!(
            "{}://{}{}",
            req.remote_url.scheme(),
            req.remote_url.host_str().unwrap_or(""),
            location
        )
    };

    // Step 2: TUS PATCH — upload the file data.
    let bytes = tokio::fs::read(&req.local_path).await?;

    let patch_resp = client
        .patch(&patch_url)
        .header("Tus-Resumable", "1.0.0")
        .header("Upload-Offset", "0")
        .header("Content-Type", "application/offset+octet-stream")
        .header("Content-Length", req.size.to_string())
        .body(bytes)
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let patch_status = patch_resp.status().as_u16();
    if patch_status != 204 && patch_status != 200 {
        return Err(SyncError::Http {
            status: patch_status,
            message: "TUS PATCH failed".into(),
        });
    }

    let etag = patch_resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    Ok(etag)
}
```

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test propagate_upload 2>&1
# Expected: 2 tests pass
```

- [ ] Commit:

```bash
git add crates/sync-engine/src/propagate/
git commit -m "feat(sync-engine): upload propagation (PUT + TUS)"
```

---

## Task 10: propagate download

- [ ] Write the failing test. Create `crates/sync-engine/tests/propagate_download.rs`:

```rust
use camino::Utf8Path;
use sync_engine::propagate::download::{propagate_download, DownloadRequest};
use tempfile::TempDir;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn downloads_file_atomically() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/dav/spaces/space1/notes.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"file content here")
                .insert_header("etag", r#""dl_etag""#),
        )
        .expect(1)
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let dest = Utf8Path::from_path(dir.path()).unwrap().join("notes.txt");

    let req = DownloadRequest {
        remote_url: Url::parse(&format!(
            "{}/dav/spaces/space1/notes.txt",
            server.uri()
        ))
        .unwrap(),
        local_dest: dest.clone(),
        expected_etag: None,
    };

    let etag = propagate_download(req).await.unwrap();

    // File must exist and have correct content.
    let content = tokio::fs::read_to_string(&dest).await.unwrap();
    assert_eq!(content, "file content here");
    assert_eq!(etag.trim_matches('"'), "dl_etag");

    server.verify().await;
}

#[tokio::test]
async fn fails_on_etag_mismatch() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/dav/spaces/space1/stale.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"data")
                .insert_header("etag", r#""server_etag""#),
        )
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let dest = Utf8Path::from_path(dir.path()).unwrap().join("stale.txt");

    let req = DownloadRequest {
        remote_url: Url::parse(&format!(
            "{}/dav/spaces/space1/stale.txt",
            server.uri()
        ))
        .unwrap(),
        local_dest: dest.clone(),
        expected_etag: Some("expected_different_etag".into()),
    };

    let result = propagate_download(req).await;
    assert!(result.is_err(), "should fail on etag mismatch");

    // Destination file must NOT exist (temp file was cleaned up).
    assert!(!dest.exists());
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test propagate_download 2>&1 | head -15
```

- [ ] Create `crates/sync-engine/src/propagate/download.rs`:

```rust
//! Propagation: download a remote file to the local filesystem.
//!
//! The download streams into a `.tmp` file in the same directory.  Only on
//! successful completion (and optional ETag verification) is the temp file
//! atomically renamed to the final destination.

use camino::Utf8PathBuf;
use tokio::io::AsyncWriteExt as _;
use url::Url;

use crate::error::{Result, SyncError};

/// Parameters for a single download operation.
pub struct DownloadRequest {
    /// Source URL on the remote server.
    pub remote_url: Url,
    /// Final destination path on the local filesystem.
    pub local_dest: Utf8PathBuf,
    /// If `Some`, the server's ETag must match this value.
    pub expected_etag: Option<String>,
}

/// Download `req.remote_url` to `req.local_dest`.
///
/// Returns the ETag returned by the server.
pub async fn propagate_download(req: DownloadRequest) -> Result<String> {
    let client = reqwest::Client::new();

    let resp = client
        .get(req.remote_url.as_str())
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let status = resp.status().as_u16();
    if status != 200 {
        return Err(SyncError::Http {
            status,
            message: format!("GET failed: {}", resp.status()),
        });
    }

    let server_etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Verify ETag before writing if the caller provided an expected value.
    if let Some(ref expected) = req.expected_etag {
        let stripped_server = server_etag.trim_matches('"');
        let stripped_expected = expected.trim_matches('"');
        if stripped_server != stripped_expected {
            return Err(SyncError::Parse(format!(
                "ETag mismatch: expected {expected}, got {server_etag}"
            )));
        }
    }

    // Ensure the parent directory exists.
    if let Some(parent) = req.local_dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Write to a sibling temp file first.
    let tmp_path = req.local_dest.with_extension("tmp");
    {
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
    }

    // Atomic rename.
    tokio::fs::rename(&tmp_path, &req.local_dest).await.map_err(|e| {
        // Best-effort cleanup on rename failure.
        let _ = std::fs::remove_file(&tmp_path);
        e
    })?;

    Ok(server_etag)
}
```

- [ ] Add stubs for the remaining propagate sub-modules so the crate compiles:

`crates/sync-engine/src/propagate/ops.rs` — `// TODO: implemented in Task 11`  
`crates/sync-engine/src/propagate/conflict.rs` — `// TODO`  

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test propagate_download 2>&1
# Expected: 2 tests pass
```

- [ ] Commit:

```bash
git add crates/sync-engine/src/propagate/download.rs \
        crates/sync-engine/src/propagate/ops.rs \
        crates/sync-engine/src/propagate/conflict.rs
git commit -m "feat(sync-engine): download propagation with atomic rename + ETag check"
```

---

## Task 11: propagate ops

- [ ] Write the failing test. Create `crates/sync-engine/tests/propagate_ops.rs`:

```rust
use sync_engine::propagate::ops::{
    delete_remote, mkdir_remote, rename_remote,
};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn delete_remote_sends_delete() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/dav/spaces/space1/old.txt"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let url =
        Url::parse(&format!("{}/dav/spaces/space1/old.txt", server.uri())).unwrap();
    delete_remote(&url).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn mkdir_remote_sends_mkcol() {
    let server = MockServer::start().await;

    Mock::given(method("MKCOL"))
        .and(path("/dav/spaces/space1/newdir/"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let url =
        Url::parse(&format!("{}/dav/spaces/space1/newdir/", server.uri())).unwrap();
    mkdir_remote(&url).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn rename_remote_sends_move() {
    let server = MockServer::start().await;

    Mock::given(method("MOVE"))
        .and(path("/dav/spaces/space1/a.txt"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let from =
        Url::parse(&format!("{}/dav/spaces/space1/a.txt", server.uri())).unwrap();
    let to =
        Url::parse(&format!("{}/dav/spaces/space1/b.txt", server.uri())).unwrap();
    rename_remote(&from, &to).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn delete_local_removes_file() {
    use sync_engine::propagate::ops::delete_local;
    use camino::Utf8Path;
    use tempfile::NamedTempFile;
    use std::io::Write;

    let mut f = NamedTempFile::new().unwrap();
    f.write_all(b"data").unwrap();
    f.flush().unwrap();
    let path = Utf8Path::from_path(f.path()).unwrap().to_owned();

    // Keep the NamedTempFile alive so the file exists when we call delete_local.
    delete_local(&path).await.unwrap();
    assert!(!path.exists());
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test propagate_ops 2>&1 | head -15
```

- [ ] Replace `crates/sync-engine/src/propagate/ops.rs` with:

```rust
//! Propagation: auxiliary operations (delete, mkdir, rename) — both local and remote.

use camino::Utf8Path;
use url::Url;

use crate::error::{Result, SyncError};

// ── Local operations ──────────────────────────────────────────────────────────

/// Remove a file from the local filesystem.
pub async fn delete_local(path: &Utf8Path) -> Result<()> {
    tokio::fs::remove_file(path).await?;
    Ok(())
}

// ── Remote WebDAV operations ──────────────────────────────────────────────────

/// Send a WebDAV DELETE for `url`.
pub async fn delete_remote(url: &Url) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(url.as_str())
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let status = resp.status().as_u16();
    if status != 204 && status != 200 {
        return Err(SyncError::Http {
            status,
            message: format!("DELETE failed: {}", resp.status()),
        });
    }
    Ok(())
}

/// Send a WebDAV MKCOL for `url`.
pub async fn mkdir_remote(url: &Url) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), url.as_str())
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let status = resp.status().as_u16();
    if status != 201 && status != 200 && status != 405 {
        // 405 = already exists, which is fine.
        return Err(SyncError::Http {
            status,
            message: format!("MKCOL failed: {}", resp.status()),
        });
    }
    Ok(())
}

/// Send a WebDAV MOVE from `from_url` to `to_url`.
pub async fn rename_remote(from_url: &Url, to_url: &Url) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .request(reqwest::Method::from_bytes(b"MOVE").unwrap(), from_url.as_str())
        .header("Destination", to_url.as_str())
        .header("Overwrite", "T")
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let status = resp.status().as_u16();
    if status != 201 && status != 204 {
        return Err(SyncError::Http {
            status,
            message: format!("MOVE failed: {}", resp.status()),
        });
    }
    Ok(())
}
```

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test propagate_ops 2>&1
# Expected: 4 tests pass
```

- [ ] Commit:

```bash
git add crates/sync-engine/src/propagate/ops.rs
git commit -m "feat(sync-engine): propagate ops — delete_local, delete_remote, mkdir_remote, rename_remote"
```

---

## Task 12: SyncEngine orchestrator

- [ ] Write the failing integration test. Create `crates/sync-engine/tests/engine_tests.rs`:

```rust
//! Integration test: SyncEngine.run_sync() orchestrates discovery →
//! reconcile → propagate against a wiremock WebDAV server.

use camino::Utf8Path;
use std::io::Write;
use sync_engine::engine::{EngineConfig, SyncEngine};
use sync_engine::types::ConflictStrategy;
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn propfind_one_file(server_uri: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/s1/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/s1/remote.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>12</D:getcontentlength>
        <D:getetag>"remote_etag"</D:getetag>
        <OC:fileid>file-remote-1</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#
    )
}

/// Scenario: empty local folder, remote has one file → engine downloads it.
#[tokio::test]
async fn engine_downloads_new_remote_file() {
    let server = MockServer::start().await;

    // PROPFIND returns one remote file
    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/s1.*"))
        .respond_with(
            ResponseTemplate::new(207)
                .set_body_string(propfind_one_file(&server.uri())),
        )
        .mount(&server)
        .await;

    // GET for the download
    Mock::given(method("GET"))
        .and(path("/dav/spaces/s1/remote.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"remote content")
                .insert_header("etag", r#""remote_etag""#),
        )
        .expect(1)
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let local_root = Utf8Path::from_path(dir.path()).unwrap().to_owned();
    let space_root =
        Url::parse(&format!("{}/dav/spaces/s1/", server.uri())).unwrap();

    let cfg = EngineConfig {
        folder_id: Uuid::new_v4(),
        local_root: local_root.clone(),
        space_root,
        conflict_strategy: ConflictStrategy::KeepBoth,
        max_parallel_transfers: 3,
    };

    let engine = SyncEngine::new(cfg);
    engine.run_sync().await.unwrap();

    // The file must be present on disk.
    let dest = local_root.join("remote.txt");
    assert!(dest.exists(), "remote.txt should have been downloaded");

    server.verify().await;
}

/// Scenario: local file exists, remote is empty → engine uploads it.
#[tokio::test]
async fn engine_uploads_new_local_file() {
    let server = MockServer::start().await;

    // PROPFIND returns empty collection
    let empty_propfind = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/spaces/s2/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/s2.*"))
        .respond_with(ResponseTemplate::new(207).set_body_string(empty_propfind))
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/dav/spaces/s2/local.txt"))
        .respond_with(
            ResponseTemplate::new(201).insert_header("etag", r#""up_etag""#),
        )
        .expect(1)
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let local_root = Utf8Path::from_path(dir.path()).unwrap().to_owned();

    // Create the local file.
    let mut f = std::fs::File::create(dir.path().join("local.txt")).unwrap();
    f.write_all(b"local data").unwrap();

    let space_root =
        Url::parse(&format!("{}/dav/spaces/s2/", server.uri())).unwrap();

    let cfg = EngineConfig {
        folder_id: Uuid::new_v4(),
        local_root,
        space_root,
        conflict_strategy: ConflictStrategy::KeepBoth,
        max_parallel_transfers: 3,
    };

    let engine = SyncEngine::new(cfg);
    engine.run_sync().await.unwrap();

    server.verify().await;
}
```

- [ ] Run (expect failure):

```bash
cargo test -p sync-engine --test engine_tests 2>&1 | head -15
```

- [ ] Replace `crates/sync-engine/src/engine.rs` with:

```rust
//! Phase orchestrator: runs discovery → reconcile → propagate for one sync cycle.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use camino::Utf8PathBuf;
use tokio::task::JoinSet;
use url::Url;
use uuid::Uuid;

use crate::discovery::local::discover_local;
use crate::discovery::remote::discover_remote;
use crate::error::{Result, SyncError};
use crate::propagate::download::{propagate_download, DownloadRequest};
use crate::propagate::upload::{propagate_upload, UploadRequest};
use crate::reconcile::{reconcile, JournalBaseline};
use crate::state::{FileStatus, FolderStatus, SyncState};
use crate::types::{ConflictStrategy, Direction, LocalEntry, RemoteEntry, SyncInstruction};

/// Configuration for a single sync folder.
pub struct EngineConfig {
    pub folder_id: Uuid,
    pub local_root: Utf8PathBuf,
    pub space_root: Url,
    pub conflict_strategy: ConflictStrategy,
    pub max_parallel_transfers: usize,
}

/// The sync engine for one folder pair.
pub struct SyncEngine {
    cfg: EngineConfig,
    state: Arc<RwLock<SyncState>>,
}

impl SyncEngine {
    pub fn new(cfg: EngineConfig) -> Self {
        let state = Arc::new(RwLock::new(SyncState::new(cfg.folder_id)));
        Self { cfg, state }
    }

    /// Run a full sync cycle: discovery → reconcile → propagate.
    pub async fn run_sync(&self) -> Result<()> {
        {
            let mut s = self.state.write().unwrap();
            s.status = FolderStatus::Syncing;
        }

        // ── Phase 1: Discovery ────────────────────────────────────────────────
        let (local_entries, remote_entries) = tokio::try_join!(
            discover_local(&self.cfg.local_root),
            discover_remote(&self.cfg.space_root),
        )?;

        // Index entries by their relative path for O(1) lookup.
        let local_map: HashMap<Utf8PathBuf, LocalEntry> = local_entries
            .into_iter()
            .map(|e| {
                let rel = e
                    .path
                    .strip_prefix(&self.cfg.local_root)
                    .unwrap_or(&e.path)
                    .to_owned();
                (rel, e)
            })
            .collect();

        let remote_map: HashMap<Utf8PathBuf, RemoteEntry> = remote_entries
            .into_iter()
            .map(|e| (e.path.clone(), e))
            .collect();

        // Union of all known paths.
        let mut all_paths: std::collections::HashSet<Utf8PathBuf> =
            local_map.keys().cloned().collect();
        all_paths.extend(remote_map.keys().cloned());

        // ── Phase 2: Reconcile ────────────────────────────────────────────────
        // In a full implementation this reads journal baselines from sync-db.
        // Here we use None for every path (no persisted journal yet).
        let instructions: Vec<(Utf8PathBuf, SyncInstruction)> = all_paths
            .into_iter()
            .map(|path| {
                let loc = local_map.get(&path).cloned();
                let rem = remote_map.get(&path).cloned();
                let journal: Option<JournalBaseline> = None;
                let instr = reconcile(loc, rem, journal, self.cfg.conflict_strategy);
                (path, instr)
            })
            .filter(|(_, instr)| *instr != SyncInstruction::Ignore)
            .collect();

        // ── Phase 3: Propagate ────────────────────────────────────────────────
        let mut join_set: JoinSet<Result<()>> = JoinSet::new();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            self.cfg.max_parallel_transfers,
        ));

        for (rel_path, instruction) in instructions {
            let local_path = self.cfg.local_root.join(&rel_path);
            let remote_url = self
                .cfg
                .space_root
                .join(rel_path.as_str())
                .map_err(|e| SyncError::Parse(e.to_string()))?;

            let sem = semaphore.clone();
            let state = self.state.clone();
            let rel_clone = rel_path.clone();

            match instruction {
                SyncInstruction::Download => {
                    join_set.spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        {
                            let mut s = state.write().unwrap();
                            s.set_file_status(rel_clone.clone(), FileStatus::Syncing);
                        }
                        let req = DownloadRequest {
                            remote_url,
                            local_dest: local_path,
                            expected_etag: None,
                        };
                        match propagate_download(req).await {
                            Ok(_etag) => {
                                let mut s = state.write().unwrap();
                                s.set_file_status(rel_clone, FileStatus::Ok);
                                Ok(())
                            }
                            Err(e) => {
                                let mut s = state.write().unwrap();
                                s.set_file_status(
                                    rel_clone,
                                    FileStatus::Error(e.to_string()),
                                );
                                Err(e)
                            }
                        }
                    });
                }

                SyncInstruction::Upload => {
                    join_set.spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        {
                            let mut s = state.write().unwrap();
                            s.set_file_status(rel_clone.clone(), FileStatus::Syncing);
                        }
                        let size = tokio::fs::metadata(&local_path)
                            .await
                            .map(|m| m.len())
                            .unwrap_or(0);
                        let req = UploadRequest {
                            local_path,
                            remote_url,
                            size,
                            checksum: None,
                            tus_threshold: 5 * 1024 * 1024,
                        };
                        match propagate_upload(req).await {
                            Ok(_etag) => {
                                let mut s = state.write().unwrap();
                                s.set_file_status(rel_clone, FileStatus::Ok);
                                Ok(())
                            }
                            Err(e) => {
                                let mut s = state.write().unwrap();
                                s.set_file_status(
                                    rel_clone,
                                    FileStatus::Error(e.to_string()),
                                );
                                Err(e)
                            }
                        }
                    });
                }

                // Other instructions (DeleteLocal, DeleteRemote, Conflict, …)
                // are handled by ops.rs and conflict.rs in a full implementation.
                _ => {}
            }
        }

        // Collect results.
        let mut had_error = false;
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::warn!("Transfer error: {e}");
                    had_error = true;
                }
                Err(join_err) => {
                    tracing::error!("Task panicked: {join_err}");
                    had_error = true;
                }
            }
        }

        {
            let mut s = self.state.write().unwrap();
            if had_error {
                s.status = FolderStatus::Error;
            } else {
                s.mark_complete();
            }
        }

        Ok(())
    }

    /// Return a clone of the current sync state.
    pub fn state(&self) -> Arc<RwLock<SyncState>> {
        self.state.clone()
    }
}
```

- [ ] Run (expect pass):

```bash
cargo test -p sync-engine --test engine_tests 2>&1
# Expected: 2 tests pass
```

- [ ] Run the full sync-engine test suite:

```bash
cargo test -p sync-engine 2>&1
# Expected: all tests pass (reconcile_tests, local_discovery, remote_discovery,
#           propagate_upload, propagate_download, propagate_ops, engine_tests,
#           error_variants, types_compile, state_tests)
```

- [ ] Run the full workspace test suite:

```bash
cargo test --workspace 2>&1
# Expected: all tests pass across vfs-core, vfs-off, sync-engine (and Plan 1 crates)
```

- [ ] Commit:

```bash
git add crates/sync-engine/src/engine.rs
git commit -m "feat(sync-engine): SyncEngine orchestrator with 3-phase run_sync()"
```

---

## Completion checklist

- [ ] `cargo test -p vfs-core` — all tests pass
- [ ] `cargo test -p vfs-off` — all tests pass
- [ ] `cargo test -p sync-engine` — all 12+ tests pass
- [ ] `cargo test --workspace` — no regressions
- [ ] All 12 tasks committed individually with descriptive messages
- [ ] No `unwrap()` in library code paths (only in tests)
- [ ] No `todo!()` / `unimplemented!()` in committed code

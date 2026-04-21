# Plan 1: Foundation — Workspace, sync-db, ocis-client

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Scaffold the Cargo workspace and implement the database journal crate (`sync-db`) and the oCIS HTTP client crate (`ocis-client`) covering WebDAV, Graph API, and OIDC authentication.

**Architecture:** Cargo workspace at repo root with all crates under `crates/`. `sync-db` wraps SQLite via `sqlx` with compile-time checked queries. `ocis-client` uses `reqwest` + `rustls` with OIDC PKCE auth flow, WebDAV operations, TUS chunked upload, and Graph API for Spaces discovery.

**Tech Stack:** Rust 2021 edition, tokio (async runtime), sqlx (SQLite, compile-time queries), reqwest + rustls (HTTP), serde + serde_json, uuid, chrono, thiserror, keyring (OS keychain), oauth2 (OIDC PKCE)

---

## Task 1: Workspace scaffold

- [ ] Create the workspace root `Cargo.toml` at the repo root:

```toml
# Cargo.toml
[workspace]
resolver = "2"
members = [
    "crates/sync-db",
    "crates/ocis-client",
]

[workspace.package]
edition = "2021"
version = "0.1.0"
authors = ["ownCloud Sync Contributors"]
license = "GPL-2.0-or-later"

[workspace.dependencies]
# async runtime
tokio = { version = "1.37", features = ["full"] }
# database
sqlx = { version = "0.7", features = ["sqlite", "runtime-tokio-rustls", "macros", "migrate", "chrono", "uuid"] }
# HTTP
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "stream", "multipart"] }
# serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
# utilities
uuid = { version = "1.8", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1.0"
# auth
oauth2 = { version = "4.4", default-features = false, features = ["reqwest"] }
keyring = { version = "2.3", features = ["linux-secret-service-rt-tokio-crypto-rust"] }
# XML parsing
quick-xml = { version = "0.31", features = ["serialize"] }
# URL
url = { version = "2.5", features = ["serde"] }
# testing
tokio-test = "0.4"
wiremock = "0.6"
tempfile = "3.10"
```

- [ ] Create the scaffold directories and stub `Cargo.toml` files:

```bash
mkdir -p crates/sync-db/src crates/sync-db/migrations crates/sync-db/tests
mkdir -p crates/ocis-client/src/auth crates/ocis-client/src/webdav crates/ocis-client/src/graph crates/ocis-client/src/tus crates/ocis-client/tests
```

- [ ] Create `crates/sync-db/Cargo.toml`:

```toml
[package]
name = "sync-db"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

[dependencies]
tokio = { workspace = true }
sqlx = { workspace = true }
serde = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tokio-test = { workspace = true }
tempfile = { workspace = true }
```

- [ ] Create `crates/ocis-client/Cargo.toml`:

```toml
[package]
name = "ocis-client"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

[dependencies]
tokio = { workspace = true }
reqwest = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }
oauth2 = { workspace = true }
keyring = { workspace = true }
quick-xml = { workspace = true }
url = { workspace = true }

[dev-dependencies]
tokio-test = { workspace = true }
wiremock = { workspace = true }
```

- [ ] Create minimal `src/lib.rs` stubs so `cargo check` passes:

```bash
# crates/sync-db/src/lib.rs
touch crates/sync-db/src/lib.rs

# crates/ocis-client/src/lib.rs
touch crates/ocis-client/src/lib.rs
```

- [ ] Verify the workspace compiles:

```bash
cargo check --workspace
```

Expected output: no errors, only possible "unused" warnings on empty lib files.

---

## Task 2: sync-db — migration and models

- [ ] Create `crates/sync-db/migrations/001_initial.sql` with the full schema:

```sql
-- crates/sync-db/migrations/001_initial.sql

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

INSERT INTO schema_version (version) VALUES (1);

CREATE TABLE IF NOT EXISTS metadata (
    path        TEXT    PRIMARY KEY NOT NULL,
    etag        TEXT,
    mtime       INTEGER,
    size        INTEGER,
    inode       INTEGER,
    file_id     TEXT,
    checksum    TEXT,
    is_virtual  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS upload_info (
    path        TEXT    PRIMARY KEY NOT NULL,
    upload_id   TEXT    NOT NULL,
    offset      INTEGER NOT NULL,
    size        INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS error_blacklist (
    path        TEXT    PRIMARY KEY NOT NULL,
    error_count INTEGER NOT NULL,
    last_error  TEXT    NOT NULL,
    retry_after INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS selective_sync (
    path TEXT PRIMARY KEY NOT NULL
);
```

- [ ] Create `crates/sync-db/src/models.rs` with all model structs:

```rust
// crates/sync-db/src/models.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a row in the `metadata` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct JournalEntry {
    pub path: String,
    pub etag: Option<String>,
    /// Unix timestamp (seconds since epoch).
    pub mtime: Option<i64>,
    pub size: Option<i64>,
    pub inode: Option<i64>,
    pub file_id: Option<String>,
    pub checksum: Option<String>,
    /// Non-zero if this is a virtual (placeholder) entry.
    pub is_virtual: i64,
}

impl JournalEntry {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            etag: None,
            mtime: None,
            size: None,
            inode: None,
            file_id: None,
            checksum: None,
            is_virtual: 0,
        }
    }

    pub fn is_virtual(&self) -> bool {
        self.is_virtual != 0
    }
}

/// Represents a row in the `upload_info` table (in-progress TUS upload state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct UploadInfo {
    pub path: String,
    pub upload_id: String,
    pub offset: i64,
    pub size: i64,
}

impl UploadInfo {
    pub fn new(path: impl Into<String>, upload_id: impl Into<String>, size: i64) -> Self {
        Self {
            path: path.into(),
            upload_id: upload_id.into(),
            offset: 0,
            size,
        }
    }
}

/// Represents a row in the `error_blacklist` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ErrorBlacklistEntry {
    pub path: String,
    pub error_count: i64,
    pub last_error: String,
    /// Unix timestamp: do not retry before this time.
    pub retry_after: i64,
}

impl ErrorBlacklistEntry {
    pub fn new(
        path: impl Into<String>,
        error_count: i64,
        last_error: impl Into<String>,
        retry_after: i64,
    ) -> Self {
        Self {
            path: path.into(),
            error_count,
            last_error: last_error.into(),
            retry_after,
        }
    }
}
```

- [ ] Create `crates/sync-db/src/error.rs`:

```rust
// crates/sync-db/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("SQLx error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("Entry not found: {0}")]
    NotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;
```

---

## Task 3: sync-db — SyncJournalDb

- [ ] Create `crates/sync-db/src/db.rs` with the full `SyncJournalDb` implementation:

```rust
// crates/sync-db/src/db.rs
use std::path::Path;
use std::sync::Arc;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{SqlitePool, Row};
use tokio::sync::Mutex;

use crate::error::{DbError, Result};
use crate::models::{ErrorBlacklistEntry, JournalEntry, UploadInfo};

/// Thread-safe handle to the sync journal SQLite database.
///
/// Internally wraps a [`SqlitePool`] (single-connection pool) behind an
/// `Arc<Mutex<…>>` so callers can clone and share the handle freely while
/// ensuring serialised writes.
#[derive(Clone, Debug)]
pub struct SyncJournalDb {
    pool: Arc<Mutex<sqlx::SqliteConnection>>,
}

impl SyncJournalDb {
    /// Open (or create) the SQLite database at `path`, run all pending
    /// migrations, and return a ready-to-use handle.
    pub async fn open(path: &Path) -> Result<Self> {
        use sqlx::ConnectOptions;
        use std::str::FromStr;

        let path_str = path.to_string_lossy();
        let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path_str))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);

        let mut conn = opts.connect().await?;

        // Run embedded migrations.
        sqlx::migrate!("./migrations").run(&mut conn).await?;

        Ok(Self {
            pool: Arc::new(Mutex::new(conn)),
        })
    }

    // -----------------------------------------------------------------------
    // metadata
    // -----------------------------------------------------------------------

    /// Fetch a single `JournalEntry` by path, or `None` if absent.
    pub async fn get_entry(&self, path: &str) -> Result<Option<JournalEntry>> {
        let mut conn = self.pool.lock().await;
        let row = sqlx::query_as::<_, JournalEntry>(
            "SELECT path, etag, mtime, size, inode, file_id, checksum, is_virtual \
             FROM metadata WHERE path = ?",
        )
        .bind(path)
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row)
    }

    /// Insert or replace a `JournalEntry`.
    pub async fn upsert_entry(&self, entry: &JournalEntry) -> Result<()> {
        let mut conn = self.pool.lock().await;
        sqlx::query(
            "INSERT INTO metadata (path, etag, mtime, size, inode, file_id, checksum, is_virtual) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(path) DO UPDATE SET \
               etag       = excluded.etag, \
               mtime      = excluded.mtime, \
               size       = excluded.size, \
               inode      = excluded.inode, \
               file_id    = excluded.file_id, \
               checksum   = excluded.checksum, \
               is_virtual = excluded.is_virtual",
        )
        .bind(&entry.path)
        .bind(&entry.etag)
        .bind(entry.mtime)
        .bind(entry.size)
        .bind(entry.inode)
        .bind(&entry.file_id)
        .bind(&entry.checksum)
        .bind(entry.is_virtual)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Delete a `JournalEntry` by path.  Returns `Ok(())` even if not found.
    pub async fn delete_entry(&self, path: &str) -> Result<()> {
        let mut conn = self.pool.lock().await;
        sqlx::query("DELETE FROM metadata WHERE path = ?")
            .bind(path)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    /// Return every entry in the metadata table, ordered by path.
    pub async fn list_entries(&self) -> Result<Vec<JournalEntry>> {
        let mut conn = self.pool.lock().await;
        let rows = sqlx::query_as::<_, JournalEntry>(
            "SELECT path, etag, mtime, size, inode, file_id, checksum, is_virtual \
             FROM metadata ORDER BY path",
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // upload_info
    // -----------------------------------------------------------------------

    /// Retrieve the in-progress upload state for `path`.
    pub async fn get_upload_info(&self, path: &str) -> Result<Option<UploadInfo>> {
        let mut conn = self.pool.lock().await;
        let row = sqlx::query_as::<_, UploadInfo>(
            "SELECT path, upload_id, offset, size FROM upload_info WHERE path = ?",
        )
        .bind(path)
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row)
    }

    /// Insert or replace the upload state for a path.
    pub async fn set_upload_info(&self, info: &UploadInfo) -> Result<()> {
        let mut conn = self.pool.lock().await;
        sqlx::query(
            "INSERT INTO upload_info (path, upload_id, offset, size) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(path) DO UPDATE SET \
               upload_id = excluded.upload_id, \
               offset    = excluded.offset, \
               size      = excluded.size",
        )
        .bind(&info.path)
        .bind(&info.upload_id)
        .bind(info.offset)
        .bind(info.size)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Remove the upload state for `path`.
    pub async fn clear_upload_info(&self, path: &str) -> Result<()> {
        let mut conn = self.pool.lock().await;
        sqlx::query("DELETE FROM upload_info WHERE path = ?")
            .bind(path)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // error_blacklist
    // -----------------------------------------------------------------------

    /// Insert or replace a blacklist entry.
    pub async fn add_blacklist(&self, entry: &ErrorBlacklistEntry) -> Result<()> {
        let mut conn = self.pool.lock().await;
        sqlx::query(
            "INSERT INTO error_blacklist (path, error_count, last_error, retry_after) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(path) DO UPDATE SET \
               error_count = excluded.error_count, \
               last_error  = excluded.last_error, \
               retry_after = excluded.retry_after",
        )
        .bind(&entry.path)
        .bind(entry.error_count)
        .bind(&entry.last_error)
        .bind(entry.retry_after)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Fetch the blacklist entry for `path`, or `None`.
    pub async fn get_blacklist(&self, path: &str) -> Result<Option<ErrorBlacklistEntry>> {
        let mut conn = self.pool.lock().await;
        let row = sqlx::query_as::<_, ErrorBlacklistEntry>(
            "SELECT path, error_count, last_error, retry_after \
             FROM error_blacklist WHERE path = ?",
        )
        .bind(path)
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row)
    }

    /// Remove the blacklist entry for `path`.
    pub async fn clear_blacklist(&self, path: &str) -> Result<()> {
        let mut conn = self.pool.lock().await;
        sqlx::query("DELETE FROM error_blacklist WHERE path = ?")
            .bind(path)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }
}
```

- [ ] Update `crates/sync-db/src/lib.rs` to expose all modules:

```rust
// crates/sync-db/src/lib.rs
pub mod db;
pub mod error;
pub mod models;

pub use db::SyncJournalDb;
pub use error::{DbError, Result};
pub use models::{ErrorBlacklistEntry, JournalEntry, UploadInfo};
```

- [ ] Verify it compiles:

```bash
cargo check -p sync-db
```

---

## Task 4: sync-db — tests

- [ ] Create `crates/sync-db/tests/db_tests.rs`:

```rust
// crates/sync-db/tests/db_tests.rs
use std::path::PathBuf;

use sync_db::{ErrorBlacklistEntry, JournalEntry, SyncJournalDb, UploadInfo};
use tempfile::tempdir;

/// Helper: open a fresh DB in a temp directory.
async fn open_temp_db() -> (SyncJournalDb, tempfile::TempDir) {
    let dir = tempdir().expect("create tempdir");
    let db_path = dir.path().join("test.db");
    let db = SyncJournalDb::open(&db_path)
        .await
        .expect("open db");
    (db, dir)
}

// ---------------------------------------------------------------------------
// open / migrations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_open_creates_db() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("journal.db");
    assert!(!db_path.exists());
    let _db = SyncJournalDb::open(&db_path).await.unwrap();
    assert!(db_path.exists(), "DB file should have been created");
}

#[tokio::test]
async fn test_open_is_idempotent() {
    let (db, dir) = open_temp_db().await;
    let db_path = dir.path().join("test.db");
    // Opening the same file again must not error.
    let _db2 = SyncJournalDb::open(&db_path).await.unwrap();
}

// ---------------------------------------------------------------------------
// JournalEntry (metadata table)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_upsert_and_get_entry() {
    let (db, _dir) = open_temp_db().await;

    let entry = JournalEntry {
        path: "/Documents/hello.txt".to_string(),
        etag: Some("abc123".to_string()),
        mtime: Some(1_700_000_000),
        size: Some(42),
        inode: Some(99),
        file_id: Some("file-id-001".to_string()),
        checksum: Some("sha256:deadbeef".to_string()),
        is_virtual: 0,
    };

    db.upsert_entry(&entry).await.unwrap();

    let fetched = db.get_entry("/Documents/hello.txt").await.unwrap();
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.etag, Some("abc123".to_string()));
    assert_eq!(fetched.size, Some(42));
    assert_eq!(fetched.file_id, Some("file-id-001".to_string()));
}

#[tokio::test]
async fn test_get_entry_not_found() {
    let (db, _dir) = open_temp_db().await;
    let result = db.get_entry("/nonexistent/path").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_upsert_updates_existing_entry() {
    let (db, _dir) = open_temp_db().await;

    let mut entry = JournalEntry::new("/file.txt");
    entry.etag = Some("v1".to_string());
    db.upsert_entry(&entry).await.unwrap();

    entry.etag = Some("v2".to_string());
    entry.size = Some(100);
    db.upsert_entry(&entry).await.unwrap();

    let fetched = db.get_entry("/file.txt").await.unwrap().unwrap();
    assert_eq!(fetched.etag, Some("v2".to_string()));
    assert_eq!(fetched.size, Some(100));
}

#[tokio::test]
async fn test_delete_entry() {
    let (db, _dir) = open_temp_db().await;

    let entry = JournalEntry::new("/to-delete.txt");
    db.upsert_entry(&entry).await.unwrap();
    assert!(db.get_entry("/to-delete.txt").await.unwrap().is_some());

    db.delete_entry("/to-delete.txt").await.unwrap();
    assert!(db.get_entry("/to-delete.txt").await.unwrap().is_none());
}

#[tokio::test]
async fn test_delete_nonexistent_entry_is_ok() {
    let (db, _dir) = open_temp_db().await;
    db.delete_entry("/does-not-exist.txt").await.unwrap();
}

#[tokio::test]
async fn test_list_entries_ordered() {
    let (db, _dir) = open_temp_db().await;

    for name in &["z.txt", "a.txt", "m.txt"] {
        db.upsert_entry(&JournalEntry::new(format!("/{name}"))).await.unwrap();
    }

    let entries = db.list_entries().await.unwrap();
    let paths: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["/a.txt", "/m.txt", "/z.txt"]);
}

// ---------------------------------------------------------------------------
// UploadInfo
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_set_and_get_upload_info() {
    let (db, _dir) = open_temp_db().await;

    let info = UploadInfo::new("/big-file.bin", "upload-abc-123", 1024 * 1024 * 50);
    db.set_upload_info(&info).await.unwrap();

    let fetched = db.get_upload_info("/big-file.bin").await.unwrap();
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.upload_id, "upload-abc-123");
    assert_eq!(fetched.offset, 0);
    assert_eq!(fetched.size, 1024 * 1024 * 50);
}

#[tokio::test]
async fn test_get_upload_info_not_found() {
    let (db, _dir) = open_temp_db().await;
    assert!(db.get_upload_info("/missing").await.unwrap().is_none());
}

#[tokio::test]
async fn test_set_upload_info_updates_offset() {
    let (db, _dir) = open_temp_db().await;

    let mut info = UploadInfo::new("/file.bin", "uid-1", 1000);
    db.set_upload_info(&info).await.unwrap();

    info.offset = 512;
    db.set_upload_info(&info).await.unwrap();

    let fetched = db.get_upload_info("/file.bin").await.unwrap().unwrap();
    assert_eq!(fetched.offset, 512);
}

#[tokio::test]
async fn test_clear_upload_info() {
    let (db, _dir) = open_temp_db().await;

    let info = UploadInfo::new("/f", "uid", 10);
    db.set_upload_info(&info).await.unwrap();
    db.clear_upload_info("/f").await.unwrap();
    assert!(db.get_upload_info("/f").await.unwrap().is_none());
}

// ---------------------------------------------------------------------------
// ErrorBlacklist
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_add_and_get_blacklist() {
    let (db, _dir) = open_temp_db().await;

    let entry = ErrorBlacklistEntry::new("/bad-file.txt", 3, "403 Forbidden", 1_800_000_000);
    db.add_blacklist(&entry).await.unwrap();

    let fetched = db.get_blacklist("/bad-file.txt").await.unwrap();
    assert!(fetched.is_some());
    let fetched = fetched.unwrap();
    assert_eq!(fetched.error_count, 3);
    assert_eq!(fetched.last_error, "403 Forbidden");
    assert_eq!(fetched.retry_after, 1_800_000_000);
}

#[tokio::test]
async fn test_get_blacklist_not_found() {
    let (db, _dir) = open_temp_db().await;
    assert!(db.get_blacklist("/clean-file.txt").await.unwrap().is_none());
}

#[tokio::test]
async fn test_add_blacklist_upserts() {
    let (db, _dir) = open_temp_db().await;

    let entry = ErrorBlacklistEntry::new("/f", 1, "first error", 100);
    db.add_blacklist(&entry).await.unwrap();

    let updated = ErrorBlacklistEntry::new("/f", 2, "second error", 200);
    db.add_blacklist(&updated).await.unwrap();

    let fetched = db.get_blacklist("/f").await.unwrap().unwrap();
    assert_eq!(fetched.error_count, 2);
    assert_eq!(fetched.last_error, "second error");
}

#[tokio::test]
async fn test_clear_blacklist() {
    let (db, _dir) = open_temp_db().await;

    let entry = ErrorBlacklistEntry::new("/f", 1, "err", 999);
    db.add_blacklist(&entry).await.unwrap();
    db.clear_blacklist("/f").await.unwrap();
    assert!(db.get_blacklist("/f").await.unwrap().is_none());
}
```

- [ ] Run the tests (they should all pass):

```bash
cargo test -p sync-db -- --test-output immediate
```

Expected output:

```
running 15 tests
test test_open_creates_db ... ok
test test_open_is_idempotent ... ok
test test_upsert_and_get_entry ... ok
test test_get_entry_not_found ... ok
test test_upsert_updates_existing_entry ... ok
test test_delete_entry ... ok
test test_delete_nonexistent_entry_is_ok ... ok
test test_list_entries_ordered ... ok
test test_set_and_get_upload_info ... ok
test test_get_upload_info_not_found ... ok
test test_set_upload_info_updates_offset ... ok
test test_clear_upload_info ... ok
test test_add_and_get_blacklist ... ok
test test_get_blacklist_not_found ... ok
test test_add_blacklist_upserts ... ok
test test_clear_blacklist ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

---

## Task 5: ocis-client — error types and Cargo.toml

- [ ] The `crates/ocis-client/Cargo.toml` was already created in Task 1. Verify it contains all required dependencies (reqwest, oauth2, keyring, quick-xml, url, wiremock for dev).

- [ ] Create `crates/ocis-client/src/error.rs`:

```rust
// crates/ocis-client/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OcisError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("WebDAV error: {0}")]
    WebDav(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Keychain error: {0}")]
    Keychain(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, OcisError>;
```

- [ ] Update `crates/ocis-client/src/lib.rs`:

```rust
// crates/ocis-client/src/lib.rs
pub mod auth;
pub mod error;
pub mod graph;
pub mod tus;
pub mod webdav;

pub use error::{OcisError, Result};
```

- [ ] Verify:

```bash
cargo check -p ocis-client
```

---

## Task 6: ocis-client — OIDC auth

- [ ] Create `crates/ocis-client/src/auth/mod.rs`:

```rust
// crates/ocis-client/src/auth/mod.rs
pub mod keychain;
pub mod oidc;

pub use keychain::KeychainStore;
pub use oidc::{OidcAuth, OidcConfig, PkceVerifier, TokenSet};
```

- [ ] Create `crates/ocis-client/src/auth/oidc.rs`:

```rust
// crates/ocis-client/src/auth/oidc.rs
//! OIDC PKCE authentication flow for oCIS.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{OcisError, Result};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Configuration obtained from the OIDC discovery document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcConfig {
    pub issuer_url: Url,
    pub client_id: String,
    pub redirect_uri: Url,
    /// Authorization endpoint from discovery.
    pub authorization_endpoint: Url,
    /// Token endpoint from discovery.
    pub token_endpoint: Url,
}

/// Opaque PKCE code verifier — must be kept secret until code exchange.
#[derive(Debug, Clone)]
pub struct PkceVerifier(pub(crate) String);

/// The token set returned after a successful OIDC exchange or refresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix timestamp (seconds) when `access_token` expires.
    pub expires_at: i64,
}

impl TokenSet {
    /// Returns `true` if the access token has expired (or expires within 30 s).
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        now >= self.expires_at - 30
    }
}

// ---------------------------------------------------------------------------
// Discovery document (subset of fields we care about)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DiscoveryDocument {
    issuer: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
}

// ---------------------------------------------------------------------------
// Token response from the /token endpoint
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

// ---------------------------------------------------------------------------
// OidcAuth
// ---------------------------------------------------------------------------

/// Stateless helper for performing OIDC PKCE flows against an oCIS server.
#[derive(Debug, Clone)]
pub struct OidcAuth {
    config: OidcConfig,
    http: Client,
}

impl OidcAuth {
    /// Fetch the OIDC discovery document from `{issuer}/.well-known/openid-configuration`
    /// and build an [`OidcAuth`] instance.
    ///
    /// `client_id` and `redirect_uri` are application-specific values that are
    /// not present in the discovery document.
    pub async fn discover(
        issuer: &str,
        client_id: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Result<Self> {
        let issuer_url: Url = issuer
            .parse()
            .map_err(|e: url::ParseError| OcisError::Auth(e.to_string()))?;

        // Build the discovery URL: strip trailing slash then append the well-known path.
        let discovery_url = issuer_url
            .join(".well-known/openid-configuration")
            .map_err(|e| OcisError::Auth(e.to_string()))?;

        let http = Client::builder()
            .use_rustls_tls()
            .build()
            .map_err(OcisError::Http)?;

        let doc: DiscoveryDocument = http
            .get(discovery_url)
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?
            .json()
            .await
            .map_err(OcisError::Http)?;

        let redirect_uri_str = redirect_uri.into();
        let redirect_uri_url: Url = redirect_uri_str
            .parse()
            .map_err(|e: url::ParseError| OcisError::Auth(e.to_string()))?;

        let config = OidcConfig {
            issuer_url: issuer_url.clone(),
            client_id: client_id.into(),
            redirect_uri: redirect_uri_url,
            authorization_endpoint: doc.authorization_endpoint,
            token_endpoint: doc.token_endpoint,
        };

        Ok(Self { config, http })
    }

    /// Build an authorization URL for the PKCE flow.
    ///
    /// Returns `(authorization_url, verifier)`.  The caller must open
    /// `authorization_url` in a browser and later pass `verifier` to
    /// [`exchange_code`].
    pub fn start_pkce_flow(&self) -> Result<(Url, PkceVerifier)> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use sha2::{Digest, Sha256};

        // Generate a cryptographically random 32-byte verifier.
        let mut raw = [0u8; 32];
        getrandom::getrandom(&mut raw)
            .map_err(|e| OcisError::Auth(format!("RNG failure: {e}")))?;

        let verifier = URL_SAFE_NO_PAD.encode(raw);

        // code_challenge = BASE64URL-ENCODE(SHA256(ASCII(verifier)))
        let hash = Sha256::digest(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(&hash[..]);

        let state = {
            let mut raw_state = [0u8; 16];
            getrandom::getrandom(&mut raw_state)
                .map_err(|e| OcisError::Auth(format!("RNG failure: {e}")))?;
            URL_SAFE_NO_PAD.encode(raw_state)
        };

        let mut auth_url = self.config.authorization_endpoint.clone();
        {
            let mut q = auth_url.query_pairs_mut();
            q.append_pair("response_type", "code");
            q.append_pair("client_id", &self.config.client_id);
            q.append_pair("redirect_uri", self.config.redirect_uri.as_str());
            q.append_pair("scope", "openid profile email offline_access");
            q.append_pair("code_challenge", &challenge);
            q.append_pair("code_challenge_method", "S256");
            q.append_pair("state", &state);
        }

        Ok((auth_url, PkceVerifier(verifier)))
    }

    /// Exchange an authorization `code` (received at the redirect URI) for a
    /// [`TokenSet`] using the provided PKCE `verifier`.
    pub async fn exchange_code(&self, code: &str, verifier: PkceVerifier) -> Result<TokenSet> {
        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.config.redirect_uri.as_str()),
            ("client_id", self.config.client_id.as_str()),
            ("code_verifier", verifier.0.as_str()),
        ];

        let resp: TokenResponse = self
            .http
            .post(self.config.token_endpoint.clone())
            .form(&params)
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?
            .json()
            .await
            .map_err(OcisError::Http)?;

        Ok(token_response_to_set(resp))
    }

    /// Use a `refresh_token` to obtain a fresh [`TokenSet`].
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenSet> {
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", self.config.client_id.as_str()),
        ];

        let resp: TokenResponse = self
            .http
            .post(self.config.token_endpoint.clone())
            .form(&params)
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?
            .json()
            .await
            .map_err(OcisError::Http)?;

        Ok(token_response_to_set(resp))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn token_response_to_set(resp: TokenResponse) -> TokenSet {
    let expires_at = {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        now + resp.expires_in.unwrap_or(3600) as i64
    };

    TokenSet {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at,
    }
}
```

- [ ] Add the two additional dependencies the OIDC module needs to `crates/ocis-client/Cargo.toml`:

```toml
# in [dependencies]
base64 = "0.22"
sha2 = "0.10"
getrandom = "0.2"
```

- [ ] Verify:

```bash
cargo check -p ocis-client
```

---

## Task 7: ocis-client — keychain storage

- [ ] Create `crates/ocis-client/src/auth/keychain.rs`:

```rust
// crates/ocis-client/src/auth/keychain.rs
//! OS keychain integration for storing OIDC token sets.

use keyring::Entry;
use serde_json;

use crate::auth::oidc::TokenSet;
use crate::error::{OcisError, Result};

/// The keyring service name used for all entries.
const SERVICE_NAME: &str = "owncloud-sync";

/// Thin wrapper around the OS keychain for persisting [`TokenSet`] values.
pub struct KeychainStore;

impl KeychainStore {
    /// Serialize `token_set` as JSON and store it under `account_id`.
    pub fn save(account_id: &str, token_set: &TokenSet) -> Result<()> {
        let json = serde_json::to_string(token_set).map_err(OcisError::Json)?;
        let entry = Entry::new(SERVICE_NAME, account_id)
            .map_err(|e| OcisError::Keychain(e.to_string()))?;
        entry
            .set_password(&json)
            .map_err(|e| OcisError::Keychain(e.to_string()))
    }

    /// Load and deserialize the [`TokenSet`] for `account_id`, or `None` if
    /// no entry exists.
    pub fn load(account_id: &str) -> Result<Option<TokenSet>> {
        let entry = Entry::new(SERVICE_NAME, account_id)
            .map_err(|e| OcisError::Keychain(e.to_string()))?;

        match entry.get_password() {
            Ok(json) => {
                let token_set: TokenSet =
                    serde_json::from_str(&json).map_err(OcisError::Json)?;
                Ok(Some(token_set))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(OcisError::Keychain(e.to_string())),
        }
    }

    /// Delete the stored entry for `account_id`.  Returns `Ok(())` if the
    /// entry did not exist.
    pub fn delete(account_id: &str) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, account_id)
            .map_err(|e| OcisError::Keychain(e.to_string()))?;

        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(OcisError::Keychain(e.to_string())),
        }
    }
}
```

- [ ] Verify:

```bash
cargo check -p ocis-client
```

---

## Task 8: ocis-client — WebDAV PROPFIND parser

- [ ] Create `crates/ocis-client/src/webdav/mod.rs`:

```rust
// crates/ocis-client/src/webdav/mod.rs
pub mod propfind;

pub use propfind::{parse_propfind_response, DavEntry, ResourceType};
```

- [ ] Create `crates/ocis-client/src/webdav/propfind.rs`:

```rust
// crates/ocis-client/src/webdav/propfind.rs
//! Parser for WebDAV PROPFIND (multistatus) XML responses.

use chrono::{DateTime, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{OcisError, Result};

// DAV namespace prefix we normalise to in the parser state machine.
const NS_DAV: &[u8] = b"DAV:";
const NS_OC: &[u8] = b"http://owncloud.org/ns";

/// Whether a DAV resource is a file or a collection (directory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceType {
    File,
    Directory,
}

/// A single entry from a PROPFIND multistatus response.
#[derive(Debug, Clone)]
pub struct DavEntry {
    pub href: String,
    pub etag: Option<String>,
    pub last_modified: Option<DateTime<Utc>>,
    pub content_length: Option<u64>,
    pub resource_type: ResourceType,
    /// oCIS file-ID from the `oc:fileid` property.
    pub file_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Parser state machine
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct CurrentEntry {
    href: Option<String>,
    etag: Option<String>,
    last_modified: Option<DateTime<Utc>>,
    content_length: Option<u64>,
    resource_type: ResourceType,
    file_id: Option<String>,
    // Are we inside a <D:propstat> that has a non-200 status?
    propstat_ok: bool,
    // Which text-element are we currently collecting?
    collecting: Collecting,
}

impl Default for ResourceType {
    fn default() -> Self {
        ResourceType::File
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
enum Collecting {
    #[default]
    None,
    Href,
    Etag,
    LastModified,
    ContentLength,
    Collection, // inside <D:collection/> means it IS a directory
    FileId,
    Status,
}

/// Parse a WebDAV PROPFIND multistatus XML body and return one [`DavEntry`]
/// per `<D:response>` element.
pub fn parse_propfind_response(xml: &str) -> Result<Vec<DavEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut entries: Vec<DavEntry> = Vec::new();
    let mut current: Option<CurrentEntry> = None;
    let mut depth: u32 = 0; // nesting depth relative to <D:response>
    let mut in_response = false;
    let mut status_text = String::new();

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let (ns, local) = split_name(e.name().as_ref());

                if ns == NS_DAV && local == b"response" {
                    in_response = true;
                    current = Some(CurrentEntry::default());
                    depth = 0;
                    continue;
                }

                if !in_response {
                    buf.clear();
                    continue;
                }

                depth += 1;

                if let Some(ref mut c) = current {
                    match (ns, local) {
                        (n, b"href") if n == NS_DAV => c.collecting = Collecting::Href,
                        (n, b"getetag") if n == NS_DAV => c.collecting = Collecting::Etag,
                        (n, b"getlastmodified") if n == NS_DAV => {
                            c.collecting = Collecting::LastModified
                        }
                        (n, b"getcontentlength") if n == NS_DAV => {
                            c.collecting = Collecting::ContentLength
                        }
                        (n, b"collection") if n == NS_DAV => {
                            c.resource_type = ResourceType::Directory;
                        }
                        (n, b"fileid") if n == NS_OC => c.collecting = Collecting::FileId,
                        (n, b"status") if n == NS_DAV => {
                            c.collecting = Collecting::Status;
                            status_text.clear();
                        }
                        _ => {}
                    }
                }
            }

            Ok(Event::Empty(ref e)) => {
                let (ns, local) = split_name(e.name().as_ref());
                if in_response && ns == NS_DAV && local == b"collection" {
                    if let Some(ref mut c) = current {
                        c.resource_type = ResourceType::Directory;
                    }
                }
            }

            Ok(Event::End(ref e)) => {
                let (ns, local) = split_name(e.name().as_ref());

                if ns == NS_DAV && local == b"response" {
                    if let Some(entry) = current.take() {
                        if let Some(href) = entry.href {
                            entries.push(DavEntry {
                                href,
                                etag: entry.etag,
                                last_modified: entry.last_modified,
                                content_length: entry.content_length,
                                resource_type: entry.resource_type,
                                file_id: entry.file_id,
                            });
                        }
                    }
                    in_response = false;
                    continue;
                }

                if !in_response {
                    buf.clear();
                    continue;
                }

                if let Some(ref mut c) = current {
                    c.collecting = Collecting::None;
                }
                if depth > 0 {
                    depth -= 1;
                }
            }

            Ok(Event::Text(ref e)) => {
                if !in_response {
                    buf.clear();
                    continue;
                }

                if let Some(ref mut c) = current {
                    let text = match e.unescape() {
                        Ok(t) => t.trim().to_string(),
                        Err(_) => {
                            buf.clear();
                            continue;
                        }
                    };
                    if text.is_empty() {
                        buf.clear();
                        continue;
                    }

                    match c.collecting {
                        Collecting::Href => c.href = Some(text),
                        Collecting::Etag => {
                            c.etag = Some(text.trim_matches('"').to_string())
                        }
                        Collecting::LastModified => {
                            if let Ok(dt) =
                                DateTime::parse_from_rfc2822(&text)
                            {
                                c.last_modified = Some(dt.with_timezone(&Utc));
                            }
                        }
                        Collecting::ContentLength => {
                            if let Ok(n) = text.parse::<u64>() {
                                c.content_length = Some(n);
                            }
                        }
                        Collecting::FileId => c.file_id = Some(text),
                        Collecting::Status => {
                            status_text = text;
                        }
                        _ => {}
                    }
                }
            }

            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(OcisError::Parse(format!("XML parse error: {e}")));
            }
            _ => {}
        }

        buf.clear();
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split a qualified XML name `{namespace}local` into `(namespace, local)`.
///
/// `quick-xml` with namespace resolution returns names like `{DAV:}response`.
/// Here we handle both resolved and plain names robustly.
fn split_name(name: &[u8]) -> (&[u8], &[u8]) {
    // quick-xml may give us just the local name when namespaces aren't
    // resolved.  We do a simple linear scan.
    if let Some(pos) = name.iter().position(|&b| b == b'}') {
        (&name[1..pos], &name[pos + 1..])
    } else {
        // No namespace — return empty namespace.
        (b"", name)
    }
}
```

- [ ] Verify:

```bash
cargo check -p ocis-client
```

---

## Task 9: ocis-client — WebDavClient

- [ ] Extend `crates/ocis-client/src/webdav/mod.rs` with the full client:

```rust
// crates/ocis-client/src/webdav/mod.rs  (replace entire file)
pub mod propfind;

pub use propfind::{parse_propfind_response, DavEntry, ResourceType};

use std::sync::Arc;

use reqwest::{header, Client, Method, StatusCode};
use tokio::sync::RwLock;
use url::Url;

use crate::auth::oidc::TokenSet;
use crate::error::{OcisError, Result};

// ---------------------------------------------------------------------------
// WebDavClient
// ---------------------------------------------------------------------------

/// HTTP client for WebDAV operations against an oCIS server.
#[derive(Debug, Clone)]
pub struct WebDavClient {
    pub base_url: Url,
    client: Client,
    token: Arc<RwLock<TokenSet>>,
}

impl WebDavClient {
    /// Create a new client.  `token` is shared so callers can update it after
    /// a token refresh.
    pub fn new(base_url: Url, token: Arc<RwLock<TokenSet>>) -> Self {
        let client = Client::builder()
            .use_rustls_tls()
            .build()
            .expect("build reqwest client");
        Self { base_url, client, token }
    }

    // -----------------------------------------------------------------------
    // Core request helper
    // -----------------------------------------------------------------------

    /// Issue a request, automatically refreshing the token once on 401.
    ///
    /// `build_request` receives a fresh `Client` and should return a
    /// `reqwest::RequestBuilder`.  The closure is called twice in the
    /// retry-on-401 case.
    async fn request_with_retry(
        &self,
        method: Method,
        url: Url,
        setup: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let access_token = self.token.read().await.access_token.clone();
        let req = setup(
            self.client
                .request(method.clone(), url.clone())
                .bearer_auth(&access_token),
        );

        let resp = req.send().await.map_err(OcisError::Http)?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            // Token has been updated externally — retry with the new one.
            let new_token = self.token.read().await.access_token.clone();
            let retry = setup(
                self.client
                    .request(method, url)
                    .bearer_auth(&new_token),
            );
            return retry.send().await.map_err(OcisError::Http);
        }

        Ok(resp)
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// PROPFIND `depth=1` — list a collection.
    pub async fn propfind(&self, path: &str) -> Result<Vec<DavEntry>> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;

        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:oc="http://owncloud.org/ns">
  <D:prop>
    <D:getetag/>
    <D:getlastmodified/>
    <D:getcontentlength/>
    <D:resourcetype/>
    <oc:fileid/>
  </D:prop>
</D:propfind>"#;

        let resp = self
            .request_with_retry(Method::from_bytes(b"PROPFIND").unwrap(), url, |req| {
                req.header("Depth", "1")
                    .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
                    .body(body)
            })
            .await?;

        if !resp.status().is_success() && resp.status().as_u16() != 207 {
            return Err(OcisError::WebDav(format!(
                "PROPFIND failed: {}",
                resp.status()
            )));
        }

        let text = resp.text().await.map_err(OcisError::Http)?;
        parse_propfind_response(&text)
    }

    /// GET — download a file, returns the raw bytes.
    pub async fn get(&self, path: &str) -> Result<bytes::Bytes> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        let resp = self
            .request_with_retry(Method::GET, url, |req| req)
            .await?
            .error_for_status()
            .map_err(OcisError::Http)?;

        resp.bytes().await.map_err(OcisError::Http)
    }

    /// PUT — upload a file body.  `content_length` must match the body size.
    pub async fn put(
        &self,
        path: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<()> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        let len = body.len() as u64;

        self.request_with_retry(Method::PUT, url, |req| {
            req.header(header::CONTENT_TYPE, content_type)
                .header(header::CONTENT_LENGTH, len)
                .body(body.clone())
        })
        .await?
        .error_for_status()
        .map_err(OcisError::Http)?;

        Ok(())
    }

    /// DELETE — remove a resource.
    pub async fn delete(&self, path: &str) -> Result<()> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        self.request_with_retry(Method::DELETE, url, |req| req)
            .await?
            .error_for_status()
            .map_err(OcisError::Http)?;
        Ok(())
    }

    /// MKCOL — create a collection (directory).
    pub async fn mkcol(&self, path: &str) -> Result<()> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        self.request_with_retry(Method::from_bytes(b"MKCOL").unwrap(), url, |req| req)
            .await?
            .error_for_status()
            .map_err(OcisError::Http)?;
        Ok(())
    }

    /// MOVE — rename or move a resource.
    ///
    /// `destination` must be an absolute path on the same server.
    /// `overwrite` maps to the `Overwrite: T/F` header.
    pub async fn move_(
        &self,
        source: &str,
        destination: &str,
        overwrite: bool,
    ) -> Result<()> {
        let url = self.base_url.join(source).map_err(OcisError::Url)?;
        let dest_url = self.base_url.join(destination).map_err(OcisError::Url)?;
        let overwrite_value = if overwrite { "T" } else { "F" };

        self.request_with_retry(Method::from_bytes(b"MOVE").unwrap(), url, |req| {
            req.header("Destination", dest_url.as_str())
                .header("Overwrite", overwrite_value)
        })
        .await?
        .error_for_status()
        .map_err(OcisError::Http)?;

        Ok(())
    }
}
```

- [ ] Add `bytes` to `crates/ocis-client/Cargo.toml`:

```toml
bytes = "1.6"
```

- [ ] Verify:

```bash
cargo check -p ocis-client
```

---

## Task 10: ocis-client — Graph API (Spaces)

- [ ] Create `crates/ocis-client/src/graph/mod.rs`:

```rust
// crates/ocis-client/src/graph/mod.rs
//! oCIS Graph API client — Spaces (Drives) discovery.

use std::sync::Arc;

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use url::Url;

use crate::auth::oidc::TokenSet;
use crate::error::{OcisError, Result};

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Quota information for a Space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceQuota {
    pub total: i64,
    pub used: i64,
    pub remaining: i64,
}

/// An oCIS Space (Drive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Space {
    pub id: String,
    pub name: String,
    #[serde(rename = "driveType")]
    pub drive_type: String,
    #[serde(rename = "webUrl")]
    pub web_url: String,
    pub quota: Option<SpaceQuota>,
}

// ---------------------------------------------------------------------------
// JSON deserialization helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DriveListResponse {
    value: Vec<DriveJson>,
}

#[derive(Debug, Deserialize)]
struct DriveJson {
    id: String,
    name: String,
    #[serde(rename = "driveType", default)]
    drive_type: String,
    #[serde(rename = "webUrl", default)]
    web_url: String,
    quota: Option<QuotaJson>,
}

#[derive(Debug, Deserialize)]
struct QuotaJson {
    total: Option<i64>,
    used: Option<i64>,
    remaining: Option<i64>,
}

impl From<DriveJson> for Space {
    fn from(d: DriveJson) -> Self {
        Space {
            id: d.id,
            name: d.name,
            drive_type: d.drive_type,
            web_url: d.web_url,
            quota: d.quota.map(|q| SpaceQuota {
                total: q.total.unwrap_or(0),
                used: q.used.unwrap_or(0),
                remaining: q.remaining.unwrap_or(0),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// GraphClient
// ---------------------------------------------------------------------------

/// HTTP client for the oCIS Graph API.
#[derive(Debug, Clone)]
pub struct GraphClient {
    pub base_url: Url,
    client: Client,
    token: Arc<RwLock<TokenSet>>,
}

impl GraphClient {
    /// Create a new client.  `base_url` should be the oCIS server root
    /// (e.g. `https://ocis.example.com`).
    pub fn new(base_url: Url, token: Arc<RwLock<TokenSet>>) -> Self {
        let client = Client::builder()
            .use_rustls_tls()
            .build()
            .expect("build reqwest client");
        Self { base_url, client, token }
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        let token = self.token.read().await.access_token.clone();

        let resp = self
            .client
            .get(url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?;

        resp.json::<T>().await.map_err(OcisError::Http)
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// List all Spaces (Drives) accessible to the current user.
    ///
    /// Calls `GET /graph/v1.0/me/drives`.
    pub async fn list_spaces(&self) -> Result<Vec<Space>> {
        let resp: DriveListResponse = self.get_json("graph/v1.0/me/drives").await?;
        Ok(resp.value.into_iter().map(Space::from).collect())
    }

    /// Fetch a single Space by its Drive ID.
    ///
    /// Calls `GET /graph/v1.0/drives/{driveId}`.
    pub async fn get_space(&self, drive_id: &str) -> Result<Space> {
        let path = format!("graph/v1.0/drives/{drive_id}");
        let drive: DriveJson = self.get_json(&path).await?;
        Ok(Space::from(drive))
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Return the WebDAV base URL for the given `space_id` on `server_url`.
///
/// Example: `https://ocis.example.com/dav/spaces/storage$personal!abc-123/`
pub fn webdav_url_for_space(server_url: &Url, space_id: &str) -> Result<Url> {
    let path = format!("dav/spaces/{space_id}/");
    server_url.join(&path).map_err(OcisError::Url)
}
```

- [ ] Verify:

```bash
cargo check -p ocis-client
```

---

## Task 11: ocis-client — TUS chunked upload

- [ ] Create `crates/ocis-client/src/tus/mod.rs`:

```rust
// crates/ocis-client/src/tus/mod.rs
//! TUS 1.0 resumable upload protocol implementation.
//!
//! Spec: <https://tus.io/protocols/resumable-upload.html>

use std::collections::HashMap;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use reqwest::{Client, StatusCode};
use tokio::sync::RwLock;
use url::Url;

use crate::auth::oidc::TokenSet;
use crate::error::{OcisError, Result};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// State for a single TUS upload session.
#[derive(Debug, Clone)]
pub struct TusUpload {
    pub upload_url: Url,
    pub offset: u64,
    pub total_size: u64,
}

// ---------------------------------------------------------------------------
// TusClient
// ---------------------------------------------------------------------------

/// TUS resumable upload client for oCIS.
#[derive(Debug, Clone)]
pub struct TusClient {
    client: Client,
    token: Arc<RwLock<TokenSet>>,
}

impl TusClient {
    pub fn new(token: Arc<RwLock<TokenSet>>) -> Self {
        let client = Client::builder()
            .use_rustls_tls()
            .build()
            .expect("build reqwest client");
        Self { client, token }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Initiate a new TUS upload.
    ///
    /// POST to `endpoint` with the required TUS headers.  The server responds
    /// with a `Location` header pointing at the upload URL.
    ///
    /// `metadata` is encoded as base64 key-value pairs in the
    /// `Upload-Metadata` header.
    pub async fn create(
        &self,
        endpoint: &Url,
        path: &str,
        size: u64,
        metadata: HashMap<String, String>,
    ) -> Result<TusUpload> {
        let token = self.token.read().await.access_token.clone();

        // Encode metadata as "key base64value, ..."
        let meta_header: String = {
            let mut pairs: Vec<String> = metadata
                .iter()
                .map(|(k, v)| format!("{} {}", k, BASE64_STANDARD.encode(v)))
                .collect();
            // Always include the filename as "filename <base64(path)>".
            pairs.push(format!("filename {}", BASE64_STANDARD.encode(path)));
            pairs.join(", ")
        };

        let resp = self
            .client
            .post(endpoint.clone())
            .bearer_auth(&token)
            .header("Tus-Resumable", "1.0.0")
            .header("Upload-Length", size.to_string())
            .header("Upload-Metadata", meta_header)
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .send()
            .await
            .map_err(OcisError::Http)?;

        if resp.status() != StatusCode::CREATED {
            return Err(OcisError::WebDav(format!(
                "TUS create failed: {}",
                resp.status()
            )));
        }

        let location = resp
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| OcisError::WebDav("TUS create: missing Location header".into()))?;

        let upload_url: Url = location
            .parse()
            .map_err(|e: url::ParseError| OcisError::WebDav(e.to_string()))?;

        Ok(TusUpload {
            upload_url,
            offset: 0,
            total_size: size,
        })
    }

    /// Upload a chunk of data.
    ///
    /// PATCH to `upload.upload_url` with the `data` bytes starting at
    /// `upload.offset`.  On success `upload.offset` is advanced by `data.len()`.
    pub async fn upload_chunk(
        &self,
        upload: &mut TusUpload,
        data: &[u8],
    ) -> Result<()> {
        let token = self.token.read().await.access_token.clone();

        let resp = self
            .client
            .patch(upload.upload_url.clone())
            .bearer_auth(&token)
            .header("Tus-Resumable", "1.0.0")
            .header("Content-Type", "application/offset+octet-stream")
            .header("Upload-Offset", upload.offset.to_string())
            .header(reqwest::header::CONTENT_LENGTH, data.len().to_string())
            .body(data.to_vec())
            .send()
            .await
            .map_err(OcisError::Http)?;

        if resp.status() != StatusCode::NO_CONTENT {
            return Err(OcisError::WebDav(format!(
                "TUS upload_chunk failed: {}",
                resp.status()
            )));
        }

        // Read the new server-side offset from the response header.
        let server_offset = resp
            .headers()
            .get("Upload-Offset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(upload.offset + data.len() as u64);

        upload.offset = server_offset;
        Ok(())
    }

    /// Resume an interrupted upload by querying the current server offset.
    ///
    /// HEAD to `upload_url`, reads the `Upload-Offset` header.
    pub async fn resume(&self, upload_url: &Url) -> Result<TusUpload> {
        let token = self.token.read().await.access_token.clone();

        let resp = self
            .client
            .head(upload_url.clone())
            .bearer_auth(&token)
            .header("Tus-Resumable", "1.0.0")
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?;

        let offset = resp
            .headers()
            .get("Upload-Offset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| OcisError::WebDav("TUS resume: missing Upload-Offset header".into()))?;

        let total_size = resp
            .headers()
            .get("Upload-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        Ok(TusUpload {
            upload_url: upload_url.clone(),
            offset,
            total_size,
        })
    }
}
```

- [ ] Verify:

```bash
cargo check -p ocis-client
```

---

## Task 12: ocis-client — integration tests

- [ ] Create `crates/ocis-client/tests/webdav_tests.rs`:

```rust
// crates/ocis-client/tests/webdav_tests.rs
//! Integration tests for WebDavClient using wiremock.

use std::sync::Arc;

use tokio::sync::RwLock;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ocis_client::auth::oidc::TokenSet;
use ocis_client::webdav::{ResourceType, WebDavClient};

fn dummy_token() -> Arc<RwLock<TokenSet>> {
    Arc::new(RwLock::new(TokenSet {
        access_token: "test-access-token".into(),
        refresh_token: None,
        expires_at: i64::MAX,
    }))
}

const PROPFIND_RESPONSE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:oc="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/personal/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getetag>"dir-etag-001"</D:getetag>
        <oc:fileid>dir-file-id-001</oc:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/personal/hello.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getetag>"abc123"</D:getetag>
        <D:getcontentlength>42</D:getcontentlength>
        <D:getlastmodified>Mon, 01 Jan 2024 12:00:00 GMT</D:getlastmodified>
        <oc:fileid>file-id-001</oc:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

#[tokio::test]
async fn test_propfind_parses_multistatus() {
    let server = MockServer::start().await;

    Mock::given(method("PROPFIND"))
        .and(path("/dav/spaces/personal/"))
        .and(header("Depth", "1"))
        .respond_with(
            ResponseTemplate::new(207)
                .set_body_raw(PROPFIND_RESPONSE, "application/xml; charset=utf-8"),
        )
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = WebDavClient::new(base_url, dummy_token());

    let entries = client.propfind("dav/spaces/personal/").await.unwrap();
    assert_eq!(entries.len(), 2);

    let dir = &entries[0];
    assert_eq!(dir.href, "/dav/spaces/personal/");
    assert_eq!(dir.resource_type, ResourceType::Directory);
    assert_eq!(dir.file_id.as_deref(), Some("dir-file-id-001"));

    let file = &entries[1];
    assert_eq!(file.href, "/dav/spaces/personal/hello.txt");
    assert_eq!(file.resource_type, ResourceType::File);
    assert_eq!(file.etag.as_deref(), Some("abc123"));
    assert_eq!(file.content_length, Some(42));
    assert_eq!(file.file_id.as_deref(), Some("file-id-001"));
}

#[tokio::test]
async fn test_propfind_retries_on_401() {
    let server = MockServer::start().await;

    // First request returns 401.
    Mock::given(method("PROPFIND"))
        .and(path("/dav/spaces/personal/"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // Second request (retry) returns 207.
    Mock::given(method("PROPFIND"))
        .and(path("/dav/spaces/personal/"))
        .respond_with(
            ResponseTemplate::new(207)
                .set_body_raw(PROPFIND_RESPONSE, "application/xml; charset=utf-8"),
        )
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let token = dummy_token();
    let client = WebDavClient::new(base_url, token.clone());

    // Simulate an external token refresh before the test so the retry succeeds.
    {
        let mut t = token.write().await;
        t.access_token = "refreshed-token".into();
    }

    let entries = client.propfind("dav/spaces/personal/").await.unwrap();
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn test_delete_sends_correct_method() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/dav/spaces/personal/old.txt"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = WebDavClient::new(base_url, dummy_token());
    client.delete("dav/spaces/personal/old.txt").await.unwrap();
}

#[tokio::test]
async fn test_mkcol_creates_directory() {
    let server = MockServer::start().await;

    Mock::given(method("MKCOL"))
        .and(path("/dav/spaces/personal/new-dir/"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = WebDavClient::new(base_url, dummy_token());
    client.mkcol("dav/spaces/personal/new-dir/").await.unwrap();
}

#[tokio::test]
async fn test_move_sets_destination_header() {
    let server = MockServer::start().await;

    Mock::given(method("MOVE"))
        .and(path("/dav/spaces/personal/old.txt"))
        .and(header("Overwrite", "T"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = WebDavClient::new(base_url, dummy_token());
    client
        .move_("dav/spaces/personal/old.txt", "dav/spaces/personal/new.txt", true)
        .await
        .unwrap();
}
```

- [ ] Create `crates/ocis-client/tests/graph_tests.rs`:

```rust
// crates/ocis-client/tests/graph_tests.rs
//! Integration tests for GraphClient using wiremock.

use std::sync::Arc;

use tokio::sync::RwLock;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ocis_client::auth::oidc::TokenSet;
use ocis_client::graph::{webdav_url_for_space, GraphClient};

fn dummy_token() -> Arc<RwLock<TokenSet>> {
    Arc::new(RwLock::new(TokenSet {
        access_token: "test-token".into(),
        refresh_token: None,
        expires_at: i64::MAX,
    }))
}

const LIST_SPACES_RESPONSE: &str = r#"{
  "value": [
    {
      "id": "storage-personal-abc123",
      "name": "Personal",
      "driveType": "personal",
      "webUrl": "https://ocis.example.com/personal",
      "quota": {
        "total": 10737418240,
        "used": 104857600,
        "remaining": 10632560640
      }
    },
    {
      "id": "storage-project-xyz",
      "name": "Project Alpha",
      "driveType": "project",
      "webUrl": "https://ocis.example.com/drives/project-alpha",
      "quota": null
    }
  ]
}"#;

const GET_SPACE_RESPONSE: &str = r#"{
  "id": "storage-personal-abc123",
  "name": "Personal",
  "driveType": "personal",
  "webUrl": "https://ocis.example.com/personal",
  "quota": {
    "total": 10737418240,
    "used": 104857600,
    "remaining": 10632560640
  }
}"#;

#[tokio::test]
async fn test_list_spaces_parses_json() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/graph/v1.0/me/drives"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(LIST_SPACES_RESPONSE, "application/json"),
        )
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = GraphClient::new(base_url, dummy_token());

    let spaces = client.list_spaces().await.unwrap();
    assert_eq!(spaces.len(), 2);

    let personal = &spaces[0];
    assert_eq!(personal.id, "storage-personal-abc123");
    assert_eq!(personal.name, "Personal");
    assert_eq!(personal.drive_type, "personal");
    let quota = personal.quota.as_ref().unwrap();
    assert_eq!(quota.total, 10737418240);
    assert_eq!(quota.used, 104857600);

    let project = &spaces[1];
    assert_eq!(project.id, "storage-project-xyz");
    assert!(project.quota.is_none());
}

#[tokio::test]
async fn test_get_space_by_id() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/graph/v1.0/drives/storage-personal-abc123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(GET_SPACE_RESPONSE, "application/json"),
        )
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = GraphClient::new(base_url, dummy_token());

    let space = client.get_space("storage-personal-abc123").await.unwrap();
    assert_eq!(space.name, "Personal");
}

#[tokio::test]
async fn test_webdav_url_for_space() {
    let server_url: url::Url = "https://ocis.example.com/".parse().unwrap();
    let space_id = "storage$personal!abc-123";
    let url = webdav_url_for_space(&server_url, space_id).unwrap();
    assert_eq!(
        url.as_str(),
        "https://ocis.example.com/dav/spaces/storage$personal!abc-123/"
    );
}
```

- [ ] Create `crates/ocis-client/tests/tus_tests.rs`:

```rust
// crates/ocis-client/tests/tus_tests.rs
//! Integration tests for TusClient using wiremock.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ocis_client::auth::oidc::TokenSet;
use ocis_client::tus::TusClient;

fn dummy_token() -> Arc<RwLock<TokenSet>> {
    Arc::new(RwLock::new(TokenSet {
        access_token: "tus-test-token".into(),
        refresh_token: None,
        expires_at: i64::MAX,
    }))
}

#[tokio::test]
async fn test_tus_create_returns_upload_state() {
    let server = MockServer::start().await;
    let upload_url = format!("{}/tus/uploads/abc-upload-id", server.uri());

    Mock::given(method("POST"))
        .and(path("/tus/files/"))
        .and(header("Tus-Resumable", "1.0.0"))
        .and(header("Upload-Length", "1024"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("Location", upload_url.as_str())
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    let client = TusClient::new(dummy_token());
    let endpoint: url::Url = format!("{}/tus/files/", server.uri()).parse().unwrap();

    let mut metadata = HashMap::new();
    metadata.insert("content-type".to_string(), "text/plain".to_string());

    let upload = client
        .create(&endpoint, "/documents/report.txt", 1024, metadata)
        .await
        .unwrap();

    assert_eq!(upload.offset, 0);
    assert_eq!(upload.total_size, 1024);
    assert!(upload.upload_url.as_str().contains("abc-upload-id"));
}

#[tokio::test]
async fn test_tus_upload_chunk_updates_offset() {
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/tus/uploads/abc-upload-id"))
        .and(header("Tus-Resumable", "1.0.0"))
        .and(header("Upload-Offset", "0"))
        .respond_with(
            ResponseTemplate::new(204)
                .insert_header("Upload-Offset", "512")
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    let client = TusClient::new(dummy_token());
    let upload_url: url::Url = format!("{}/tus/uploads/abc-upload-id", server.uri())
        .parse()
        .unwrap();

    let mut upload = ocis_client::tus::TusUpload {
        upload_url,
        offset: 0,
        total_size: 1024,
    };

    let data = vec![0u8; 512];
    client.upload_chunk(&mut upload, &data).await.unwrap();

    assert_eq!(upload.offset, 512);
}

#[tokio::test]
async fn test_tus_resume_reads_offset_from_head() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/tus/uploads/abc-upload-id"))
        .and(header("Tus-Resumable", "1.0.0"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Upload-Offset", "256")
                .insert_header("Upload-Length", "1024")
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    let client = TusClient::new(dummy_token());
    let upload_url: url::Url = format!("{}/tus/uploads/abc-upload-id", server.uri())
        .parse()
        .unwrap();

    let upload = client.resume(&upload_url).await.unwrap();

    assert_eq!(upload.offset, 256);
    assert_eq!(upload.total_size, 1024);
    assert_eq!(upload.upload_url, upload_url);
}

#[tokio::test]
async fn test_tus_full_sequence() {
    let server = MockServer::start().await;
    let upload_url_str = format!("{}/tus/uploads/full-seq-id", server.uri());

    // 1. Create
    Mock::given(method("POST"))
        .and(path("/tus/files/"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("Location", upload_url_str.as_str())
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    // 2. First chunk
    Mock::given(method("PATCH"))
        .and(path("/tus/uploads/full-seq-id"))
        .and(header("Upload-Offset", "0"))
        .respond_with(
            ResponseTemplate::new(204)
                .insert_header("Upload-Offset", "512")
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // 3. Second chunk
    Mock::given(method("PATCH"))
        .and(path("/tus/uploads/full-seq-id"))
        .and(header("Upload-Offset", "512"))
        .respond_with(
            ResponseTemplate::new(204)
                .insert_header("Upload-Offset", "1024")
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    let client = TusClient::new(dummy_token());
    let endpoint: url::Url = format!("{}/tus/files/", server.uri()).parse().unwrap();

    let mut upload = client
        .create(&endpoint, "/video.mp4", 1024, HashMap::new())
        .await
        .unwrap();

    assert_eq!(upload.offset, 0);

    let chunk1 = vec![0xABu8; 512];
    client.upload_chunk(&mut upload, &chunk1).await.unwrap();
    assert_eq!(upload.offset, 512);

    let chunk2 = vec![0xCDu8; 512];
    client.upload_chunk(&mut upload, &chunk2).await.unwrap();
    assert_eq!(upload.offset, 1024);
}
```

- [ ] Run the full ocis-client test suite:

```bash
cargo test -p ocis-client -- --test-output immediate
```

Expected output:

```
running 12 tests
test test_propfind_parses_multistatus ... ok
test test_propfind_retries_on_401 ... ok
test test_delete_sends_correct_method ... ok
test test_mkcol_creates_directory ... ok
test test_move_sets_destination_header ... ok
test test_list_spaces_parses_json ... ok
test test_get_space_by_id ... ok
test test_webdav_url_for_space ... ok
test test_tus_create_returns_upload_state ... ok
test test_tus_upload_chunk_updates_offset ... ok
test test_tus_resume_reads_offset_from_head ... ok
test test_tus_full_sequence ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

- [ ] Run the full workspace test suite and commit:

```bash
cargo test --workspace -- --test-output immediate
```

```bash
git add Cargo.toml crates/
git commit -m "feat: scaffold workspace, implement sync-db and ocis-client foundation"
```

// crates/sync-db/src/db.rs
use std::path::Path;
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use crate::error::Result;
use crate::models::{ErrorBlacklistEntry, JournalEntry, UploadInfo};

/// Thread-safe handle to the sync journal SQLite database.
///
/// Wraps a single-connection [`SqlitePool`] so the handle is cheaply cloneable
/// and can be shared across async tasks without external locking.
#[derive(Clone, Debug)]
pub struct SyncJournalDb {
    pool: SqlitePool,
}

impl SyncJournalDb {
    /// Open (or create) the SQLite database at `path`, run all pending
    /// migrations, and return a ready-to-use handle.
    ///
    /// Migrations are embedded at compile-time from `crates/sync-db/migrations/`.
    pub async fn open(path: &Path) -> Result<Self> {
        let path_str = path.to_str().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "non-UTF-8 path")
        })?;
        let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path_str))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }

    // -----------------------------------------------------------------------
    // metadata
    // -----------------------------------------------------------------------

    /// Fetch a single [`JournalEntry`] by path, or `None` if absent.
    pub async fn get_entry(&self, path: &str) -> Result<Option<JournalEntry>> {
        let row = sqlx::query_as::<_, JournalEntry>(
            "SELECT path, etag, mtime, size, inode, file_id, checksum, is_virtual \
             FROM metadata WHERE path = ?",
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Insert or replace a [`JournalEntry`].
    pub async fn upsert_entry(&self, entry: &JournalEntry) -> Result<()> {
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
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a [`JournalEntry`] by path. Returns `Ok(())` even if not found.
    pub async fn delete_entry(&self, path: &str) -> Result<()> {
        sqlx::query("DELETE FROM metadata WHERE path = ?")
            .bind(path)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Return every entry in the metadata table, ordered by path.
    pub async fn list_entries(&self) -> Result<Vec<JournalEntry>> {
        let rows = sqlx::query_as::<_, JournalEntry>(
            "SELECT path, etag, mtime, size, inode, file_id, checksum, is_virtual \
             FROM metadata ORDER BY path",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // upload_info
    // -----------------------------------------------------------------------

    /// Retrieve the in-progress TUS upload state for `path`.
    pub async fn get_upload_info(&self, path: &str) -> Result<Option<UploadInfo>> {
        let row = sqlx::query_as::<_, UploadInfo>(
            "SELECT path, upload_id, offset, size FROM upload_info WHERE path = ?",
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Insert or replace the upload state for a path.
    pub async fn set_upload_info(&self, info: &UploadInfo) -> Result<()> {
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
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove the upload state for `path`.
    pub async fn clear_upload_info(&self, path: &str) -> Result<()> {
        sqlx::query("DELETE FROM upload_info WHERE path = ?")
            .bind(path)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // error_blacklist
    // -----------------------------------------------------------------------

    /// Insert or replace a blacklist entry.
    pub async fn add_blacklist(&self, entry: &ErrorBlacklistEntry) -> Result<()> {
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
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch the blacklist entry for `path`, or `None`.
    pub async fn get_blacklist(&self, path: &str) -> Result<Option<ErrorBlacklistEntry>> {
        let row = sqlx::query_as::<_, ErrorBlacklistEntry>(
            "SELECT path, error_count, last_error, retry_after \
             FROM error_blacklist WHERE path = ?",
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Remove the blacklist entry for `path`.
    pub async fn clear_blacklist(&self, path: &str) -> Result<()> {
        sqlx::query("DELETE FROM error_blacklist WHERE path = ?")
            .bind(path)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

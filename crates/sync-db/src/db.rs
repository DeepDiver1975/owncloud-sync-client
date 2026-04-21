// crates/sync-db/src/db.rs
use std::path::Path;
use std::sync::Arc;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::ConnectOptions;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::models::{ErrorBlacklistEntry, JournalEntry, UploadInfo};

/// Thread-safe handle to the sync journal SQLite database.
#[derive(Clone, Debug)]
pub struct SyncJournalDb {
    pool: Arc<Mutex<sqlx::SqliteConnection>>,
}

impl SyncJournalDb {
    /// Open (or create) the SQLite database at `path`, run all pending
    /// migrations, and return a ready-to-use handle.
    pub async fn open(path: &Path) -> Result<Self> {
        use std::str::FromStr;

        let path_str = path.to_string_lossy();
        let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path_str))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);

        let mut conn = opts.connect().await?;

        sqlx::migrate!("./migrations").run(&mut conn).await?;

        Ok(Self {
            pool: Arc::new(Mutex::new(conn)),
        })
    }

    // -----------------------------------------------------------------------
    // metadata
    // -----------------------------------------------------------------------

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

    pub async fn delete_entry(&self, path: &str) -> Result<()> {
        let mut conn = self.pool.lock().await;
        sqlx::query("DELETE FROM metadata WHERE path = ?")
            .bind(path)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

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

    pub async fn clear_blacklist(&self, path: &str) -> Result<()> {
        let mut conn = self.pool.lock().await;
        sqlx::query("DELETE FROM error_blacklist WHERE path = ?")
            .bind(path)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }
}

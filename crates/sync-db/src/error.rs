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

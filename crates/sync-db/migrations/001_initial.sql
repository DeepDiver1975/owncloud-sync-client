-- crates/sync-db/migrations/001_initial.sql

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY NOT NULL
);

INSERT OR IGNORE INTO schema_version (version) VALUES (1);

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

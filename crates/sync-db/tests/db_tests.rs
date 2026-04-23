// crates/sync-db/tests/db_tests.rs
use sync_db::{ErrorBlacklistEntry, JournalEntry, SyncJournalDb, UploadInfo};
use tempfile::tempdir;

async fn open_temp_db() -> (SyncJournalDb, tempfile::TempDir) {
    let dir = tempdir().expect("create tempdir");
    let db_path = dir.path().join("test.db");
    let db = SyncJournalDb::open(&db_path).await.expect("open db");
    (db, dir)
}

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
    let _db2 = SyncJournalDb::open(&db_path).await.unwrap();
    drop(db);
}

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
    assert_eq!(fetched, Some(entry));
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
        db.upsert_entry(&JournalEntry::new(format!("/{name}")))
            .await
            .unwrap();
    }

    let entries = db.list_entries().await.unwrap();
    let paths: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["/a.txt", "/m.txt", "/z.txt"]);
}

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

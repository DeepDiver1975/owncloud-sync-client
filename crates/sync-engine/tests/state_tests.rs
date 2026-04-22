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

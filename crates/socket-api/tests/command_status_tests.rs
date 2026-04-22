use camino::Utf8PathBuf;
use socket_api::commands::status::{
    handle_retrieve_file_status, handle_retrieve_folder_status,
};
use socket_api::status_resolver::StatusResolver;
use sync_engine::state::{FileStatus, SyncState};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

fn make_resolver(root: &str, file_path: &str, status: FileStatus) -> StatusResolver {
    let folder_id = Uuid::new_v4();
    let mut state = SyncState::new(folder_id);
    state.set_file_status(Utf8PathBuf::from(file_path), status);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from(root), folder_id)];
    StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    )
}

fn make_empty_resolver() -> StatusResolver {
    StatusResolver::new(
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(RwLock::new(vec![])),
    )
}

#[test]
fn file_status_none_for_untracked_path() {
    let resolver = make_empty_resolver();
    let resp = handle_retrieve_file_status("/not/synced/file.txt", &resolver);
    assert_eq!(resp, "STATUS:NONE:/not/synced/file.txt\n");
}

#[test]
fn file_status_ok() {
    let resolver = make_resolver("/sync", "/sync/ok.txt", FileStatus::Ok);
    let resp = handle_retrieve_file_status("/sync/ok.txt", &resolver);
    assert_eq!(resp, "STATUS:OK:/sync/ok.txt\n");
}

#[test]
fn file_status_sync() {
    let resolver = make_resolver("/sync", "/sync/uploading.txt", FileStatus::Syncing);
    let resp = handle_retrieve_file_status("/sync/uploading.txt", &resolver);
    assert_eq!(resp, "STATUS:SYNC:/sync/uploading.txt\n");
}

#[test]
fn file_status_error() {
    let resolver = make_resolver(
        "/sync",
        "/sync/broken.txt",
        FileStatus::Error("network error".into()),
    );
    let resp = handle_retrieve_file_status("/sync/broken.txt", &resolver);
    assert_eq!(resp, "STATUS:ERROR:/sync/broken.txt\n");
}

#[test]
fn file_status_excluded() {
    let resolver = make_resolver("/sync", "/sync/.hidden", FileStatus::Excluded);
    let resp = handle_retrieve_file_status("/sync/.hidden", &resolver);
    assert_eq!(resp, "STATUS:EXCLUDED:/sync/.hidden\n");
}

#[test]
fn folder_status_none_for_untracked_path() {
    let resolver = make_empty_resolver();
    let resp = handle_retrieve_folder_status("/not/synced/", &resolver);
    assert_eq!(resp, "STATUS:NONE:/not/synced/\n");
}

#[test]
fn folder_status_ok_for_clean_folder() {
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from("/sync"), folder_id)];
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    );
    let resp = handle_retrieve_folder_status("/sync/subdir", &resolver);
    assert_eq!(resp, "STATUS:OK:/sync/subdir\n");
}

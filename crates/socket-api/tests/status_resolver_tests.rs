use camino::Utf8PathBuf;
use socket_api::status_resolver::StatusResolver;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use sync_engine::state::{FileStatus, SyncState};
use uuid::Uuid;

fn make_resolver_with_folder(root: &str, file_path: &str, status: FileStatus) -> StatusResolver {
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

#[test]
fn path_not_in_any_folder_returns_none() {
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(RwLock::new(vec![])),
    );
    assert_eq!(resolver.resolve_file("/not/synced/file.txt"), "NONE");
}

#[test]
fn file_with_ok_status_returns_ok() {
    let resolver = make_resolver_with_folder("/sync/root", "/sync/root/file.txt", FileStatus::Ok);
    assert_eq!(resolver.resolve_file("/sync/root/file.txt"), "OK");
}

#[test]
fn file_with_syncing_status_returns_sync() {
    let resolver = make_resolver_with_folder(
        "/sync/root",
        "/sync/root/uploading.txt",
        FileStatus::Syncing,
    );
    assert_eq!(resolver.resolve_file("/sync/root/uploading.txt"), "SYNC");
}

#[test]
fn file_with_error_status_returns_error() {
    let resolver = make_resolver_with_folder(
        "/sync/root",
        "/sync/root/broken.txt",
        FileStatus::Error("checksum mismatch".into()),
    );
    assert_eq!(resolver.resolve_file("/sync/root/broken.txt"), "ERROR");
}

#[test]
fn file_with_excluded_status_returns_excluded() {
    let resolver =
        make_resolver_with_folder("/sync/root", "/sync/root/.hidden", FileStatus::Excluded);
    assert_eq!(resolver.resolve_file("/sync/root/.hidden"), "EXCLUDED");
}

#[test]
fn file_in_folder_but_no_status_entry_returns_ok() {
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from("/sync/root"), folder_id)];
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    );
    assert_eq!(resolver.resolve_file("/sync/root/any_file.txt"), "OK");
}

#[test]
fn resolve_folder_path_in_sync_root_returns_ok() {
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from("/sync/root"), folder_id)];
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    );
    assert_eq!(resolver.resolve_folder("/sync/root/subdir"), "OK");
}

#[test]
fn find_folder_for_path_returns_correct_uuid() {
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let folder_roots = vec![(Utf8PathBuf::from("/sync/root"), folder_id)];
    let resolver = StatusResolver::new(
        Arc::new(RwLock::new(states)),
        Arc::new(RwLock::new(folder_roots)),
    );
    assert_eq!(
        resolver.find_folder_for_path("/sync/root/deep/file.txt"),
        Some(folder_id)
    );
    assert_eq!(resolver.find_folder_for_path("/outside/path"), None);
}

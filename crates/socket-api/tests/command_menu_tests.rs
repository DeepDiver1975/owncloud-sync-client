use camino::Utf8PathBuf;
use socket_api::commands::menu::{handle_get_menu_items, handle_get_strings};
use socket_api::status_resolver::StatusResolver;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use sync_engine::state::{FileStatus, SyncState};
use uuid::Uuid;

fn make_resolver_with_file(root: &str, file_path: &str, status: FileStatus) -> StatusResolver {
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
fn get_strings_contains_share_menu_title() {
    let resp = handle_get_strings();
    assert!(
        resp.contains("SHARE_MENU_TITLE"),
        "GET_STRINGS should contain SHARE_MENU_TITLE"
    );
}

#[test]
fn get_strings_starts_with_get_strings_prefix() {
    let resp = handle_get_strings();
    assert!(
        resp.starts_with("GET_STRINGS:"),
        "should start with GET_STRINGS:"
    );
}

#[test]
fn get_strings_ends_with_newline() {
    let resp = handle_get_strings();
    assert!(
        resp.ends_with('\n'),
        "GET_STRINGS response must end with newline"
    );
}

#[test]
fn get_strings_contains_make_available() {
    let resp = handle_get_strings();
    assert!(
        resp.contains("MAKE_AVAILABLE"),
        "should contain MAKE_AVAILABLE key"
    );
}

#[test]
fn get_strings_contains_make_online_only() {
    let resp = handle_get_strings();
    assert!(
        resp.contains("MAKE_ONLINE_ONLY"),
        "should contain MAKE_ONLINE_ONLY key"
    );
}

#[test]
fn get_menu_items_path_not_in_sync_folder_returns_empty() {
    let resolver = make_empty_resolver();
    let resp = handle_get_menu_items("/not/synced/file.txt", &resolver);
    assert!(
        resp.starts_with("GET_MENU_ITEMS:"),
        "should start with GET_MENU_ITEMS:"
    );
    assert!(resp.ends_with('\n'));
}

#[test]
fn get_menu_items_synced_file_includes_share_item() {
    let resolver = make_resolver_with_file("/sync", "/sync/doc.pdf", FileStatus::Ok);
    let resp = handle_get_menu_items("/sync/doc.pdf", &resolver);
    assert!(
        resp.contains("SHARE"),
        "menu for a synced file should include SHARE item"
    );
}

#[test]
fn get_menu_items_synced_file_includes_copy_link_item() {
    let resolver = make_resolver_with_file("/sync", "/sync/doc.pdf", FileStatus::Ok);
    let resp = handle_get_menu_items("/sync/doc.pdf", &resolver);
    assert!(
        resp.contains("COPY_PRIVATE_LINK"),
        "menu for a synced file should include COPY_PRIVATE_LINK item"
    );
}

#[test]
fn get_menu_items_format_has_path_first() {
    let resolver = make_resolver_with_file("/sync", "/sync/a.txt", FileStatus::Ok);
    let resp = handle_get_menu_items("/sync/a.txt", &resolver);
    assert!(
        resp.starts_with("GET_MENU_ITEMS:/sync/a.txt"),
        "path must be first field after GET_MENU_ITEMS:"
    );
}

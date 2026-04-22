use camino::Utf8PathBuf;
use socket_api::server::SocketApiServer;
use sync_engine::state::SyncState;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use vfs_off::VfsOff;

fn make_server() -> Arc<SocketApiServer> {
    let folder_id = Uuid::new_v4();
    let state = SyncState::new(folder_id);
    let mut states = HashMap::new();
    states.insert(folder_id, state);
    let sync_states = Arc::new(RwLock::new(states));
    let folder_roots = Arc::new(RwLock::new(vec![
        (Utf8PathBuf::from("/sync/root"), folder_id),
    ]));
    let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());

    Arc::new(SocketApiServer::new(sync_states, folder_roots, vfs))
}

#[test]
fn server_constructs_without_panic() {
    let _server = make_server();
}

#[tokio::test]
async fn dispatch_version_command_returns_version_string() {
    let server = make_server();
    let resp = server.dispatch_line("VERSION").await;
    assert_eq!(resp, Some("VERSION:1.1\n".to_string()));
}

#[tokio::test]
async fn dispatch_get_strings_returns_string_map() {
    let server = make_server();
    let resp = server.dispatch_line("GET_STRINGS").await;
    let resp = resp.expect("dispatch returned None");
    assert!(resp.starts_with("GET_STRINGS:"));
    assert!(resp.contains("SHARE_MENU_TITLE"));
}

#[tokio::test]
async fn dispatch_retrieve_file_status_untracked() {
    let server = make_server();
    let resp = server
        .dispatch_line("RETRIEVE_FILE_STATUS:/outside/sync.txt")
        .await;
    assert_eq!(resp, Some("STATUS:NONE:/outside/sync.txt\n".to_string()));
}

#[tokio::test]
async fn dispatch_share_returns_ok() {
    let server = make_server();
    let resp = server.dispatch_line("SHARE:/sync/root/doc.pdf").await;
    assert_eq!(resp, Some("SHARE:OK:/sync/root/doc.pdf\n".to_string()));
}

#[tokio::test]
async fn dispatch_copy_private_link_returns_ok() {
    let server = make_server();
    let resp = server
        .dispatch_line("COPY_PRIVATE_LINK:/sync/root/file.txt")
        .await;
    assert_eq!(
        resp,
        Some("COPY_PRIVATE_LINK:OK:/sync/root/file.txt\n".to_string())
    );
}

#[tokio::test]
async fn dispatch_make_available_locally_returns_ok() {
    let server = make_server();
    let resp = server
        .dispatch_line("MAKE_AVAILABLE_LOCALLY:/sync/root/file.txt")
        .await;
    assert_eq!(resp, Some("MAKE_AVAILABLE_LOCALLY:OK\n".to_string()));
}

#[tokio::test]
async fn dispatch_unknown_command_returns_none() {
    let server = make_server();
    let resp = server.dispatch_line("UNKNOWN_COMMAND:arg").await;
    assert!(resp.is_none(), "unknown command should return None (silently ignored)");
}

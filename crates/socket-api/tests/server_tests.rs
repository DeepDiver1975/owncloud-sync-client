#[cfg(unix)]
mod integration {
    use camino::Utf8PathBuf;
    use socket_api::broadcast::BroadcastSender;
    use socket_api::server::SocketApiServer;
    use socket_api::transport::unix::UnixTransport;
    use socket_api::transport::Transport;
    use sync_engine::state::SyncState;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use uuid::Uuid;
    use vfs_off::VfsOff;

    async fn start_server(
        dir: &TempDir,
    ) -> (Arc<SocketApiServer>, Arc<BroadcastSender>, std::path::PathBuf) {
        let socket_path = dir.path().join("test.sock");

        let sync_states: Arc<RwLock<HashMap<Uuid, SyncState>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let folder_roots: Arc<RwLock<Vec<(Utf8PathBuf, Uuid)>>> =
            Arc::new(RwLock::new(vec![]));
        let vfs: Arc<dyn vfs_core::Vfs> = Arc::new(VfsOff::new());

        let server = Arc::new(SocketApiServer::new(sync_states, folder_roots, vfs));
        let broadcast = server.broadcast();

        let transport = UnixTransport::bind(&socket_path).await.unwrap();
        let server_clone = server.clone();
        tokio::spawn(async move {
            let _ = server_clone.run(Box::new(transport) as Box<dyn Transport>).await;
        });

        tokio::time::sleep(Duration::from_millis(20)).await;

        (server, broadcast, socket_path)
    }

    async fn send_command(socket_path: &std::path::Path, cmd: &str) -> String {
        let mut stream = UnixStream::connect(socket_path).await.unwrap();
        stream.write_all(format!("{cmd}\n").as_bytes()).await.unwrap();

        let mut reader = BufReader::new(&mut stream);
        let mut response = String::new();
        reader.read_line(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn version_command_returns_version_1_1() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(&socket_path, "VERSION").await;
        assert_eq!(resp, "VERSION:1.1\n");
    }

    #[tokio::test]
    async fn get_strings_response_contains_share_menu_title() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(&socket_path, "GET_STRINGS").await;
        assert!(
            resp.contains("SHARE_MENU_TITLE"),
            "GET_STRINGS should contain SHARE_MENU_TITLE, got: {resp}"
        );
    }

    #[tokio::test]
    async fn retrieve_file_status_untracked_returns_none() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(
            &socket_path,
            "RETRIEVE_FILE_STATUS:/tmp/not-synced-at-all.txt",
        )
        .await;
        assert_eq!(resp, "STATUS:NONE:/tmp/not-synced-at-all.txt\n");
    }

    #[tokio::test]
    async fn make_available_locally_returns_ok() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(&socket_path, "MAKE_AVAILABLE_LOCALLY:/some/path/file.txt").await;
        assert_eq!(resp, "MAKE_AVAILABLE_LOCALLY:OK\n");
    }

    #[tokio::test]
    async fn broadcast_status_changed_reaches_connected_client() {
        let dir = TempDir::new().unwrap();
        let (_, broadcast, socket_path) = start_server(&dir).await;

        let mut stream = UnixStream::connect(&socket_path).await.unwrap();
        let mut reader = BufReader::new(&mut stream);

        // Wait for the server to accept and register the connection in the broadcaster.
        tokio::time::sleep(Duration::from_millis(50)).await;

        broadcast.status_changed("OK", "/foo/bar.txt").await;

        let mut line = String::new();
        tokio::time::timeout(Duration::from_millis(200), reader.read_line(&mut line))
            .await
            .expect("timed out waiting for broadcast")
            .unwrap();

        assert_eq!(line, "STATUS:OK:/foo/bar.txt\n");
    }

    #[tokio::test]
    async fn share_command_returns_ok() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let resp = send_command(&socket_path, "SHARE:/sync/root/document.pdf").await;
        assert_eq!(resp, "SHARE:OK:/sync/root/document.pdf\n");
    }

    #[tokio::test]
    async fn multiple_sequential_commands_on_same_connection() {
        let dir = TempDir::new().unwrap();
        let (_, _, socket_path) = start_server(&dir).await;

        let stream = UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        write_half.write_all(b"VERSION\n").await.unwrap();
        let mut line1 = String::new();
        reader.read_line(&mut line1).await.unwrap();
        assert_eq!(line1, "VERSION:1.1\n");

        write_half.write_all(b"GET_STRINGS\n").await.unwrap();
        let mut line2 = String::new();
        reader.read_line(&mut line2).await.unwrap();
        assert!(line2.starts_with("GET_STRINGS:"), "got: {line2}");
    }
}

#[cfg(unix)]
mod unix_tests {
    use socket_api::transport::unix::UnixTransport;
    use socket_api::transport::Transport;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn bind_accept_and_exchange_message() {
        let dir = TempDir::new().unwrap();
        let socket_path = dir.path().join("test.sock");

        let transport = UnixTransport::bind(&socket_path).await.unwrap();

        let path_clone = socket_path.clone();
        let client = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let mut stream = UnixStream::connect(&path_clone).await.unwrap();
            stream.write_all(b"hello\n").await.unwrap();
            let mut buf = [0u8; 32];
            let n = stream.read(&mut buf).await.unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });

        let mut conn = transport.accept().await.unwrap();
        let mut buf = [0u8; 32];
        let n = conn.read(&mut buf).await.unwrap();
        conn.write_all(&buf[..n]).await.unwrap();

        let received = client.await.unwrap();
        assert_eq!(received, "hello\n");
    }

    #[tokio::test]
    async fn socket_file_removed_on_drop() {
        let dir = TempDir::new().unwrap();
        let socket_path = dir.path().join("drop_test.sock");
        {
            let _transport = UnixTransport::bind(&socket_path).await.unwrap();
            assert!(socket_path.exists(), "socket file should exist while transport is alive");
        }
        assert!(!socket_path.exists(), "socket file should be removed on drop");
    }
}

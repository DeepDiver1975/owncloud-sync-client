use gui::spawn::{wait_for_socket, SpawnError};
use std::path::Path;

#[tokio::test]
async fn wait_for_socket_fails_when_nothing_listening() {
    let path = Path::new("/tmp/ocsync_test_nonexistent_socket_xyz.sock");
    let result = wait_for_socket(path, 2, 1).await;
    assert!(matches!(result, Err(SpawnError::Failed(_))));
}

#[tokio::test]
async fn wait_for_socket_succeeds_when_socket_exists() {
    use tokio::net::UnixListener;

    let path = std::path::PathBuf::from(format!(
        "/tmp/ocsync_test_spawn_{}.sock",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let listener = UnixListener::bind(&path).expect("bind");
    let result = wait_for_socket(&path, 3, 1).await;
    drop(listener);
    let _ = std::fs::remove_file(&path);
    assert!(result.is_ok());
}

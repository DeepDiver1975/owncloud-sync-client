use std::time::Duration;

use acceptance_test::{fixture::TestEnvironment, poll::poll_until};
use daemon::gui_ipc::protocol::DaemonEvent;

fn skip_if_no_acceptance() -> bool {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping: OCIS_ACCEPTANCE not set");
        return true;
    }
    false
}

async fn env_after_initial_sync() -> TestEnvironment {
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("initial SyncFinished not received");
    env
}

#[tokio::test]
async fn test_files_sync_down() {
    if skip_if_no_acceptance() {
        return;
    }
    let mut env = TestEnvironment::start().await.expect("start");

    env.ocis_client
        .put("hello.txt", b"hello")
        .await
        .expect("pre-seed hello.txt");

    let local_path = env.sync_dir.path().join("hello.txt");
    poll_until(
        || {
            let path = local_path.clone();
            async move { std::fs::read(&path).map(|c| c == b"hello").unwrap_or(false) }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("hello.txt did not sync down");

    let finished = env
        .daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(10),
        )
        .await;
    assert!(finished.is_some(), "SyncFinished(errors=[]) not received");
}

#[tokio::test]
async fn test_upload_new_file() {
    if skip_if_no_acceptance() {
        return;
    }
    let env = env_after_initial_sync().await;

    let local_path = env.sync_dir.path().join("upload_new.txt");
    std::fs::write(&local_path, b"new content").expect("write local file");

    poll_until(
        || async {
            env.ocis_client
                .exists("upload_new.txt")
                .await
                .unwrap_or(false)
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("upload_new.txt did not appear on remote");

    let remote = env
        .ocis_client
        .get("upload_new.txt")
        .await
        .expect("get remote");
    assert_eq!(remote.as_ref(), b"new content");
}

#[tokio::test]
async fn test_upload_changed_file() {
    if skip_if_no_acceptance() {
        return;
    }
    let env = TestEnvironment::start().await.expect("start");

    env.ocis_client
        .put("change_me.txt", b"original")
        .await
        .expect("pre-seed");

    let local_path = env.sync_dir.path().join("change_me.txt");
    poll_until(
        || {
            let p = local_path.clone();
            async move { p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("change_me.txt did not sync down");

    std::fs::write(&local_path, b"changed content").expect("overwrite local");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if env
            .ocis_client
            .get("change_me.txt")
            .await
            .map(|b| b.as_ref() == b"changed content")
            .unwrap_or(false)
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for change_me.txt remote content to update"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[tokio::test]
async fn test_conflict_resolution() {
    if skip_if_no_acceptance() {
        return;
    }
    let env = TestEnvironment::start().await.expect("start");

    env.ocis_client
        .put("conflict.txt", b"remote v1")
        .await
        .expect("pre-seed");

    let local_path = env.sync_dir.path().join("conflict.txt");
    poll_until(
        || {
            let p = local_path.clone();
            async move { p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("conflict.txt did not sync down");

    std::fs::write(&local_path, b"local v2").expect("local write");
    env.ocis_client
        .put("conflict.txt", b"remote v2")
        .await
        .expect("remote write");

    let sync_dir = env.sync_dir.path().to_owned();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let conflict_count = std::fs::read_dir(&sync_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_name().to_string_lossy().starts_with("conflict"))
                    .count()
            })
            .unwrap_or(0);
        if conflict_count >= 2 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for conflict resolution to produce two conflict.txt files"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

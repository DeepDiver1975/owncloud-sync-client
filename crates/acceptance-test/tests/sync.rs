use std::time::Duration;

use acceptance_test::{fixture::TestEnvironment, poll::poll_until};
use daemon::gui_ipc::protocol::DaemonEvent;
use sync_engine::SyncReport;

fn skip_if_no_acceptance() -> bool {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping: OCIS_ACCEPTANCE not set");
        return true;
    }
    false
}

async fn env_with_account() -> TestEnvironment {
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");
    env.add_account().await.expect("add_account");
    env
}

async fn env_after_initial_sync() -> (TestEnvironment, SyncReport) {
    let mut env = env_with_account().await;
    let event = env
        .daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("initial SyncFinished not received");

    let report = match event {
        DaemonEvent::SyncFinished {
            report: Some(r), ..
        } => r,
        _ => panic!("SyncFinished missing report"),
    };

    (env, report)
}

#[tokio::test]
async fn test_files_sync_down() {
    if skip_if_no_acceptance() {
        return;
    }
    let mut env = env_with_account().await;

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
    let (env, _report) = env_after_initial_sync().await;

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
    let env = env_with_account().await;

    env.ocis_client
        .put("change_me.txt", b"original")
        .await
        .expect("pre-seed");

    let local_path = env.sync_dir.path().join("change_me.txt");
    poll_until(
        || {
            let p = local_path.clone();
            async move { std::fs::read(&p).map(|c| c == b"original").unwrap_or(false) }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("change_me.txt did not sync down with correct content");

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
    let env = env_with_account().await;

    env.ocis_client
        .put("conflict.txt", b"remote v1")
        .await
        .expect("pre-seed");

    let local_path = env.sync_dir.path().join("conflict.txt");
    poll_until(
        || {
            let p = local_path.clone();
            async move {
                std::fs::read(&p)
                    .map(|c| c == b"remote v1")
                    .unwrap_or(false)
            }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("conflict.txt did not sync down with correct content");

    // Write both sides before any sync cycle completes so the daemon sees them as concurrent
    std::fs::write(&local_path, b"local v2").expect("local write");
    env.ocis_client
        .put("conflict.txt", b"remote v2")
        .await
        .expect("remote write");
    // Small pause to ensure the daemon's next poll sees both changes simultaneously
    tokio::time::sleep(Duration::from_millis(100)).await;

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

#[tokio::test]
async fn test_initial_sync_empty_remote() {
    if skip_if_no_acceptance() {
        return;
    }
    let (env, report) = env_after_initial_sync().await;
    // oCIS personal spaces are never truly empty — the server pre-seeds default folders.
    // Assert that every discovered remote entry was downloaded (none skipped/ignored).
    assert_eq!(
        report.downloads, report.remote_entries,
        "expected all remote entries to be downloaded: remote={}, dl={}",
        report.remote_entries, report.downloads
    );
    let local_count = std::fs::read_dir(env.sync_dir.path())
        .expect("read sync dir")
        .filter_map(|e| e.ok())
        .count();
    assert_eq!(
        local_count, report.remote_entries,
        "local file count ({local_count}) should match remote_entries ({})",
        report.remote_entries
    );
}

#[tokio::test]
async fn test_initial_sync_preseeded_remote() {
    if skip_if_no_acceptance() {
        return;
    }
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");

    env.ocis_client
        .put("file1.txt", b"content1")
        .await
        .expect("seed file1");
    env.ocis_client
        .put("file2.txt", b"content2")
        .await
        .expect("seed file2");
    env.ocis_client
        .put("file3.txt", b"content3")
        .await
        .expect("seed file3");

    env.add_account().await.expect("add_account");

    let event = env
        .daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("SyncFinished not received within 60s");

    let report = match event {
        DaemonEvent::SyncFinished {
            report: Some(r), ..
        } => r,
        _ => panic!("SyncFinished missing report"),
    };

    // oCIS personal spaces have server-seeded default files; we added 3 on top of those.
    assert!(
        report.remote_entries >= 3,
        "expected at least 3 remote entries, got {}",
        report.remote_entries
    );
    assert_eq!(
        report.downloads, report.remote_entries,
        "expected all remote entries to be downloaded: remote={}, dl={}",
        report.remote_entries, report.downloads
    );

    for (name, content) in [
        ("file1.txt", b"content1" as &[u8]),
        ("file2.txt", b"content2"),
        ("file3.txt", b"content3"),
    ] {
        let path = env.sync_dir.path().join(name);
        let actual = std::fs::read(&path).unwrap_or_else(|_| panic!("{name} not found locally"));
        assert_eq!(actual.as_slice(), content, "{name} content mismatch");
    }
}

#[tokio::test]
async fn test_upload_new_directory() {
    if skip_if_no_acceptance() {
        return;
    }
    let (env, _) = env_after_initial_sync().await;

    let local_dir = env.sync_dir.path().join("photos");
    std::fs::create_dir(&local_dir).expect("create local dir");

    poll_until(
        || async {
            env.ocis_client
                .collection_exists("photos")
                .await
                .unwrap_or(false)
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("photos/ did not appear on remote within 30s");
}

#[tokio::test]
async fn test_upload_file_in_new_subdirectory() {
    if skip_if_no_acceptance() {
        return;
    }
    let (env, _) = env_after_initial_sync().await;

    let local_dir = env.sync_dir.path().join("docs");
    std::fs::create_dir(&local_dir).expect("create docs dir");
    std::fs::write(local_dir.join("readme.txt"), b"hello docs").expect("write readme");

    poll_until(
        || async {
            env.ocis_client
                .exists("docs/readme.txt")
                .await
                .unwrap_or(false)
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("docs/readme.txt did not appear on remote");

    let content = env
        .ocis_client
        .get("docs/readme.txt")
        .await
        .expect("get content");
    assert_eq!(content.as_ref(), b"hello docs");
}

#[tokio::test]
async fn test_watch_driven_upload() {
    if skip_if_no_acceptance() {
        return;
    }
    let (env, _) = env_after_initial_sync().await;

    let local_path = env.sync_dir.path().join("watched.txt");
    std::fs::write(&local_path, b"watched content").expect("write watched.txt");

    poll_until(
        || async { env.ocis_client.exists("watched.txt").await.unwrap_or(false) },
        Duration::from_secs(10),
        Duration::from_millis(500),
    )
    .await
    .expect("watched.txt did not appear within 10s (watcher-driven sync expected)");
}

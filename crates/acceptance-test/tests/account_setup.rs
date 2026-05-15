use std::time::Duration;

use acceptance_test::{fixture::TestEnvironment, poll::poll_until};
use daemon::config::AppConfig;
use daemon::gui_ipc::protocol::DaemonEvent;

#[tokio::test]
async fn test_account_setup() {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping test_account_setup: OCIS_ACCEPTANCE not set");
        return;
    }

    let mut env = TestEnvironment::start()
        .await
        .expect("failed to start TestEnvironment");

    let callback_title = env
        .add_account()
        .await
        .expect("account setup via OIDC failed");

    assert_eq!(
        callback_title, "Successfully signed in",
        "expected success page title after OIDC login"
    );

    // Assert: config file has exactly 1 account with non-empty user_id and 1 folder.
    let config_path = env.config_dir.path().join("owncloud").join("owncloud.toml");
    let cfg = AppConfig::load(&config_path).expect("failed to load config after add_account");

    assert_eq!(cfg.account.len(), 1, "expected exactly 1 account in config");
    let account = &cfg.account[0];
    assert!(
        !account.user_id.is_empty(),
        "expected user_id to be non-empty"
    );
    assert!(
        !account.folder.is_empty(),
        "expected at least 1 folder in account after setup"
    );
    let personal = account.folder.iter().find(|f| f.display_name == "Personal");
    assert!(personal.is_some(), "expected a 'Personal' folder");
}

#[tokio::test]
async fn test_multi_space_setup_bidirectional() {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping test_multi_space_setup_bidirectional: OCIS_ACCEPTANCE not set");
        return;
    }

    let mut env = TestEnvironment::start()
        .await
        .expect("failed to start TestEnvironment");

    env.add_account()
        .await
        .expect("account setup via OIDC failed");

    // Wait for initial sync to complete.
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("initial SyncFinished not received");

    let sync_path = env.personal_sync_dir();

    // --- DOWN-SYNC: create file on server (non-ASCII + HTTP-encoding chars), verify locally ---
    // Filename includes Chinese chars and chars that require percent-encoding in WebDAV hrefs.
    let remote_name = "测试 file?name.txt";
    env.ocis_client
        .put(remote_name, b"down-sync-content")
        .await
        .expect("seed remote file");

    poll_until(
        || {
            let p = sync_path.join(remote_name);
            async move {
                std::fs::read(&p)
                    .map(|c| c == b"down-sync-content")
                    .unwrap_or(false)
            }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("remote file did not sync down within 30s");

    // --- UP-SYNC: create file locally (non-ASCII + XML-special chars), verify on server ---
    let local_name = "上传 file<test>.txt";
    let local_file = sync_path.join(local_name);
    std::fs::write(&local_file, b"up-sync-content").expect("write local file");

    poll_until(
        || async { env.ocis_client.exists(local_name).await.unwrap_or(false) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local file did not sync up to server within 30s");

    let remote_content = env
        .ocis_client
        .get(local_name)
        .await
        .expect("fetch synced file from server");
    assert_eq!(remote_content.as_ref(), b"up-sync-content");
}

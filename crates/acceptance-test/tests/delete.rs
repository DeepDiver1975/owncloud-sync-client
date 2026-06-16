// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_deletFilesFolders.
//! Bidirectional deletion: local delete propagates up; remote delete propagates down.
//! A sibling file is asserted to survive both deletions.

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
    env.add_account().await.expect("add_account");
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
async fn test_delete_file_and_folder_bidirectional() {
    if skip_if_no_acceptance() {
        return;
    }
    let env = env_after_initial_sync().await;
    let sync_dir = env.personal_sync_dir();

    // Names: non-ASCII + HTTP-encoding chars.
    let up_file = "删除 up?file.txt"; // deleted locally -> expect gone on server
    let survivor = "幸存 keep<file>.txt"; // must remain after both deletes
    let down_file = "删除 down file.txt"; // deleted on server -> expect gone locally

    // Seed all three on the server, wait for them to sync down.
    for name in [up_file, survivor, down_file] {
        env.ocis_client
            .put(name, b"seed")
            .await
            .unwrap_or_else(|_| panic!("seed {name}"));
    }
    for name in [up_file, survivor, down_file] {
        let p = sync_dir.join(name);
        poll_until(
            || {
                let p = p.clone();
                async move { p.exists() }
            },
            Duration::from_secs(30),
            Duration::from_secs(1),
        )
        .await
        .unwrap_or_else(|_| panic!("{name} did not sync down"));
    }

    // --- UP: delete locally, expect gone on server ---
    std::fs::remove_file(sync_dir.join(up_file)).expect("remove local up_file");
    poll_until(
        || async { !env.ocis_client.exists(up_file).await.unwrap_or(true) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local deletion did not propagate to server");

    // --- DOWN: delete on server, expect gone locally ---
    env.ocis_client
        .delete(down_file)
        .await
        .expect("server delete");
    poll_until(
        || {
            let p = sync_dir.join(down_file);
            async move { !p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server deletion did not propagate to client");

    // --- SURVIVOR: untouched on both ends ---
    assert!(
        env.ocis_client.exists(survivor).await.unwrap_or(false),
        "survivor must still exist on server"
    );
    assert!(
        sync_dir.join(survivor).exists(),
        "survivor must still exist locally"
    );
}

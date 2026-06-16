// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_editFiles.
//! Bidirectional edit: local overwrite propagates up; remote overwrite propagates down.

use std::time::Duration;

use acceptance_test::fixture::TestEnvironment;
use acceptance_test::poll::poll_until;
use acceptance_test::testutil::skip_if_no_acceptance;
use daemon::gui_ipc::protocol::DaemonEvent;

#[tokio::test]
async fn test_edit_file_bidirectional() {
    if skip_if_no_acceptance() {
        return;
    }
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

    let sync_dir = env.personal_sync_dir();
    let up_name = "编辑 up?file.txt"; // edited locally
    let down_name = "编辑 down<file>.txt"; // edited on server

    // Seed both, wait for them to sync down.
    env.ocis_client
        .put(up_name, b"original up")
        .await
        .expect("seed up");
    env.ocis_client
        .put(down_name, b"original down")
        .await
        .expect("seed down");
    for (name, content) in [
        (up_name, b"original up" as &[u8]),
        (down_name, b"original down"),
    ] {
        let p = sync_dir.join(name);
        poll_until(
            || {
                let p = p.clone();
                async move { std::fs::read(&p).map(|c| c == content).unwrap_or(false) }
            },
            Duration::from_secs(30),
            Duration::from_secs(1),
        )
        .await
        .unwrap_or_else(|_| panic!("{name} did not sync down with seed content"));
    }

    // --- UP: overwrite locally, expect server content updated ---
    std::fs::write(sync_dir.join(up_name), b"edited up content").expect("overwrite local");
    poll_until(
        || async {
            env.ocis_client
                .get(up_name)
                .await
                .map(|b| b.as_ref() == b"edited up content")
                .unwrap_or(false)
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local edit did not propagate to server");

    // --- DOWN: overwrite on server, expect local content updated ---
    env.ocis_client
        .put(down_name, b"edited down content")
        .await
        .expect("server overwrite");
    poll_until(
        || {
            let p = sync_dir.join(down_name);
            async move {
                std::fs::read(&p)
                    .map(|c| c == b"edited down content")
                    .unwrap_or(false)
            }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server edit did not propagate to client");
}

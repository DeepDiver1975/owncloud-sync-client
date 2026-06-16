// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_moveFilesFolders.
//! Bidirectional move/rename: local rename-out-of-subdir propagates up;
//! server-side move propagates down.

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

#[tokio::test]
async fn test_move_and_rename_bidirectional() {
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

    // --- UP: local move from a nested subdir to the sync root ---
    let sub = "子目录 a?dir"; // nested directory
    let up_src_rel = format!("{sub}/移动 up file.txt");
    let up_dst_rel = "移动 up file.txt"; // moved to root
    let nested_dir = sync_dir.join(sub);
    std::fs::create_dir_all(&nested_dir).expect("create nested dir");
    std::fs::write(nested_dir.join("移动 up file.txt"), b"move me up").expect("write nested file");

    // Wait for the nested file to reach the server before moving it.
    poll_until(
        || async { env.ocis_client.exists(&up_src_rel).await.unwrap_or(false) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("nested file did not sync up before move");

    // Move locally: nested -> root.
    std::fs::rename(sync_dir.join(&up_src_rel), sync_dir.join(up_dst_rel))
        .expect("local move to root");

    poll_until(
        || async {
            env.ocis_client.exists(up_dst_rel).await.unwrap_or(false)
                && !env.ocis_client.exists(&up_src_rel).await.unwrap_or(true)
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local move did not propagate to server (new path present, old gone)");
    let moved = env
        .ocis_client
        .get(up_dst_rel)
        .await
        .expect("get moved file");
    assert_eq!(
        moved.as_ref(),
        b"move me up",
        "moved file content preserved"
    );

    // --- DOWN: server-side rename, expect local reflects ---
    let down_old = "重命名 old file.txt";
    let down_new = "重命名 new<file>.txt";
    env.ocis_client
        .put(down_old, b"rename me on server")
        .await
        .expect("seed down_old");
    poll_until(
        || {
            let p = sync_dir.join(down_old);
            async move { p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("down_old did not sync down");

    env.ocis_client
        .move_item(down_old, down_new)
        .await
        .expect("server move_item");

    poll_until(
        || {
            let new_p = sync_dir.join(down_new);
            let old_p = sync_dir.join(down_old);
            async move { new_p.exists() && !old_p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server rename did not propagate to client");
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_syncing (pause/resume).
//! While the folder is paused, a new local file must NOT propagate to the
//! server; after resume, it must.

use std::time::Duration;

use acceptance_test::{fixture::TestEnvironment, poll::poll_until};
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};

fn skip_if_no_acceptance() -> bool {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping: OCIS_ACCEPTANCE not set");
        return true;
    }
    false
}

#[tokio::test]
async fn test_pause_blocks_then_resume_flushes() {
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

    let folder_id = env.personal_folder_id();
    let sync_dir = env.personal_sync_dir();
    let name = "暂停 paused?file.txt";

    // Pause the folder.
    env.daemon_ipc
        .send(DaemonCommand::PauseFolder { folder_id })
        .await
        .expect("send PauseFolder");

    // Confirm the daemon has applied the pause before we create the file,
    // so the negative assertion below reflects suppression — not a race.
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountStateChanged { account_id, state } if *account_id == folder_id && state == "paused"),
            Duration::from_secs(10),
        )
        .await
        .expect("paused state not confirmed");

    // Create a local file while paused.
    std::fs::write(sync_dir.join(name), b"written while paused").expect("write local file");

    // For a bounded window, the file must NOT appear on the server.
    let appeared_while_paused = poll_until(
        || async { env.ocis_client.exists(name).await.unwrap_or(false) },
        Duration::from_secs(15),
        Duration::from_secs(1),
    )
    .await
    .is_ok();
    assert!(
        !appeared_while_paused,
        "file must not upload while folder is paused"
    );

    // Resume — the pending change must now flush to the server.
    env.daemon_ipc
        .send(DaemonCommand::ResumeFolder { folder_id })
        .await
        .expect("send ResumeFolder");

    poll_until(
        || async { env.ocis_client.exists(name).await.unwrap_or(false) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("file did not upload after resume");
}

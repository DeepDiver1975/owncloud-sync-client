// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Shared helpers for acceptance-test integration tests.

use std::time::Duration;

use daemon::gui_ipc::protocol::DaemonEvent;

use crate::fixture::TestEnvironment;

/// Returns `true` (and prints a skip notice) when `OCIS_ACCEPTANCE` is unset, so
/// each `#[tokio::test]` can early-return instead of panicking in `TestEnvironment::start()`.
pub fn skip_if_no_acceptance() -> bool {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping: OCIS_ACCEPTANCE not set");
        return true;
    }
    false
}

/// Starts a `TestEnvironment` and adds the default account.
pub async fn env_with_account() -> TestEnvironment {
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");
    env.add_account().await.expect("add_account");
    env
}

/// Starts a `TestEnvironment`, adds the account, and waits for the first
/// successful `SyncFinished` (errors empty). Returns the environment.
pub async fn env_after_initial_sync() -> TestEnvironment {
    let mut env = env_with_account().await;
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("initial SyncFinished not received");
    env
}

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

/// Like [`skip_if_no_acceptance`], but for the oc10 suite. oc10 tests also rely
/// on `OCIS_ACCEPTANCE=1` (it gates `TestEnvironment::start_with`), plus their
/// own `OC10_ACCEPTANCE=1` so the (slower, separate-stack) oc10 suite only runs
/// when explicitly requested.
pub fn skip_if_no_oc10_acceptance() -> bool {
    if std::env::var("OCIS_ACCEPTANCE").is_err() || std::env::var("OC10_ACCEPTANCE").is_err() {
        eprintln!("Skipping: set OCIS_ACCEPTANCE=1 and OC10_ACCEPTANCE=1 to run oc10 tests");
        return true;
    }
    false
}

/// Starts an oc10 `TestEnvironment` and adds the bootstrap admin account.
pub async fn oc10_env_with_account() -> TestEnvironment {
    let mut env = TestEnvironment::start_oc10()
        .await
        .expect("TestEnvironment::start_oc10");
    env.add_account().await.expect("add_account");
    env
}

/// Starts an oc10 `TestEnvironment`, adds the account, and waits for the first
/// successful `SyncFinished` (errors empty). Returns the environment.
pub async fn oc10_env_after_initial_sync() -> TestEnvironment {
    let mut env = oc10_env_with_account().await;
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::SyncFinished { errors, .. } if errors.is_empty()),
            Duration::from_secs(60),
        )
        .await
        .expect("initial SyncFinished not received");
    env
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

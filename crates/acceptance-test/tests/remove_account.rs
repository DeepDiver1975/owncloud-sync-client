// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_removeAccountConnection
//! (remove the only configured account).
//!
//! Behaviour-only migration (Tier 3): the old Squish test also asserted the
//! account-setup wizard becomes visible again after removal. The new GUI has no
//! equivalent wizard surface to assert against, so that assertion is dropped and
//! recorded in the gap registry. Here we verify the daemon-side behaviour: a
//! `RemoveAccount` command drops the account, broadcasts the removal, and the
//! change persists (a freshly-subscribed client sees an empty account list).

use std::time::Duration;

use acceptance_test::testutil::{env_after_initial_sync, skip_if_no_acceptance};
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};

#[tokio::test]
async fn test_remove_only_account() {
    if skip_if_no_acceptance() {
        return;
    }
    let mut env = env_after_initial_sync().await;
    let account_id = env.account_id();

    // Sanity: the account is present before removal. A fresh subscriber receives
    // an AccountSnapshot listing exactly this account.
    let mut probe = env.connect_fresh_ipc().await.expect("connect probe ipc");
    let before = probe
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountSnapshot { .. }),
            Duration::from_secs(10),
        )
        .await
        .expect("initial AccountSnapshot not received");
    match before {
        DaemonEvent::AccountSnapshot { accounts } => {
            assert!(
                accounts.iter().any(|a| a.account_id == account_id),
                "account should be present before removal"
            );
        }
        _ => unreachable!(),
    }

    // Remove the only account.
    env.daemon_ipc
        .send(DaemonCommand::RemoveAccount { account_id })
        .await
        .expect("failed to send RemoveAccount");

    // The daemon broadcasts the removal to subscribed clients.
    let removed = env
        .daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountStateChanged { state, .. } if state == "removed"),
            Duration::from_secs(15),
        )
        .await
        .expect("AccountStateChanged{removed} not received");
    match removed {
        DaemonEvent::AccountStateChanged { account_id: id, .. } => {
            assert_eq!(id, account_id, "removed event carried wrong account_id");
        }
        _ => unreachable!(),
    }

    // The removal persisted: a freshly-subscribed client sees no accounts.
    let mut after_ipc = env.connect_fresh_ipc().await.expect("connect fresh ipc");
    let after = after_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountSnapshot { .. }),
            Duration::from_secs(10),
        )
        .await
        .expect("post-removal AccountSnapshot not received");
    match after {
        DaemonEvent::AccountSnapshot { accounts } => {
            assert!(
                accounts.is_empty(),
                "expected no accounts after removing the only account, got: {accounts:?}"
            );
        }
        _ => unreachable!(),
    }
}

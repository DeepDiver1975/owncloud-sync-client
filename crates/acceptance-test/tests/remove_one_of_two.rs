// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_removeAccountConnection
//! (remove one of two account connections).
//!
//! Requires runtime user provisioning (Tier 2). Two accounts (admin + a
//! provisioned Alice) are configured; removing Alice must leave admin intact.
//! The old Squish test also asserted GUI state after removal; the new client has
//! no equivalent surface, so this is a daemon-behaviour migration (consistent
//! with the Tier 3 `remove_account` test).

use std::time::Duration;

use acceptance_test::fixture::TestEnvironment;
use acceptance_test::provision::UserProvisioner;
use acceptance_test::testutil::skip_if_no_acceptance;
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};

#[tokio::test]
async fn test_remove_one_of_two_accounts() {
    if skip_if_no_acceptance() {
        return;
    }
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");

    let provisioner = UserProvisioner::new(env.ocis_url.clone())
        .await
        .expect("UserProvisioner::new");
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let alice_name = format!("alice-{}", &suffix[..8]);
    let alice = provisioner
        .create_user(&alice_name, "Secret123!", "Alice")
        .await
        .expect("create Alice");

    let (admin_handle, _) = env
        .add_account_as("admin", "admin")
        .await
        .expect("add admin account");
    let (alice_handle, _) = env
        .add_account_as(&alice.username, &alice.password)
        .await
        .expect("add Alice account");

    // Sanity: both accounts present before removal.
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
            assert_eq!(
                accounts.len(),
                2,
                "expected two accounts, got: {accounts:?}"
            );
        }
        _ => unreachable!(),
    }

    // Remove the second account (Alice).
    env.daemon_ipc
        .send(DaemonCommand::RemoveAccount {
            account_id: alice_handle.account_id,
        })
        .await
        .expect("failed to send RemoveAccount");

    // The daemon broadcasts the removal for the Alice account.
    let removed = env
        .daemon_ipc
        .wait_for(
            |e| {
                matches!(e, DaemonEvent::AccountStateChanged { account_id, state }
                if *account_id == alice_handle.account_id && state == "removed")
            },
            Duration::from_secs(15),
        )
        .await
        .expect("AccountStateChanged{removed} for Alice not received");
    assert!(matches!(removed, DaemonEvent::AccountStateChanged { .. }));

    // The removal persisted: a fresh subscriber sees exactly the admin account.
    let mut after = env.connect_fresh_ipc().await.expect("connect fresh ipc");
    let snapshot = after
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountSnapshot { .. }),
            Duration::from_secs(10),
        )
        .await
        .expect("post-removal AccountSnapshot not received");
    match snapshot {
        DaemonEvent::AccountSnapshot { accounts } => {
            assert_eq!(
                accounts.len(),
                1,
                "expected one account after removing Alice, got: {accounts:?}"
            );
            assert_eq!(
                accounts[0].account_id, admin_handle.account_id,
                "the surviving account must be admin"
            );
            assert!(
                accounts
                    .iter()
                    .all(|a| a.account_id != alice_handle.account_id),
                "Alice's account must be gone"
            );
        }
        _ => unreachable!(),
    }

    // Cleanup (best-effort).
    let _ = provisioner.delete_user(&alice.id).await;
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_addAccount (adding multiple
//! accounts), and subsuming the multi-account *behaviour* of test/gui/tst_activity:
//! two accounts are tracked independently and sync to their own personal spaces.
//! The Activity GUI view itself has no equivalent in the new client and is
//! recorded as a product gap in the migration design doc.
//!
//! Requires runtime user provisioning (Tier 2): a second oCIS user is created
//! via the admin Graph API, then added as a second account.

use std::time::Duration;

use acceptance_test::fixture::TestEnvironment;
use acceptance_test::ocis_client::OcisClient;
use acceptance_test::poll::poll_until;
use acceptance_test::provision::UserProvisioner;
use acceptance_test::testutil::skip_if_no_acceptance;
use daemon::gui_ipc::protocol::DaemonEvent;

#[tokio::test]
async fn test_add_multiple_accounts() {
    if skip_if_no_acceptance() {
        return;
    }
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");

    // Provision a second user (unique name so concurrent test binaries don't clash).
    let provisioner = UserProvisioner::new(env.ocis_url.clone())
        .await
        .expect("UserProvisioner::new");
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let alice_name = format!("alice-{}", &suffix[..8]);
    let alice = provisioner
        .create_user(&alice_name, "Secret123!", "Alice")
        .await
        .expect("create Alice");

    // Add both accounts on the same oCIS URL (daemon dedups by url+user_id, so
    // two distinct users is valid).
    let (admin_handle, _) = env
        .add_account_as("admin", "admin")
        .await
        .expect("add admin account");
    let (alice_handle, _) = env
        .add_account_as(&alice.username, &alice.password)
        .await
        .expect("add Alice account");

    // --- Assert two distinct accounts are persisted ---
    let mut probe = env.connect_fresh_ipc().await.expect("connect probe ipc");
    let snapshot = probe
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountSnapshot { .. }),
            Duration::from_secs(10),
        )
        .await
        .expect("AccountSnapshot not received");
    match snapshot {
        DaemonEvent::AccountSnapshot { accounts } => {
            assert!(
                accounts
                    .iter()
                    .any(|a| a.account_id == admin_handle.account_id),
                "admin account must be present, got: {accounts:?}"
            );
            assert!(
                accounts
                    .iter()
                    .any(|a| a.account_id == alice_handle.account_id),
                "Alice account must be present, got: {accounts:?}"
            );
            assert_ne!(
                admin_handle.account_id, alice_handle.account_id,
                "the two accounts must have distinct ids"
            );
        }
        _ => unreachable!(),
    }

    // --- Assert independent up-sync: each account syncs into its own space ---
    // Per-user OcisClient for server-side assertions against each personal space.
    let admin_ocis = OcisClient::from_credentials(env.ocis_url.clone(), "admin", "admin")
        .await
        .expect("admin OcisClient");
    let alice_ocis =
        OcisClient::from_credentials(env.ocis_url.clone(), &alice.username, &alice.password)
            .await
            .expect("Alice OcisClient");

    // Non-ASCII + HTTP-encoding chars; distinct content per account.
    let admin_file = "管理员 up?file.txt";
    let alice_file = "爱丽丝 up<file>.txt";
    std::fs::write(
        admin_handle.personal_sync_dir.join(admin_file),
        b"admin body",
    )
    .expect("write admin file");
    std::fs::write(
        alice_handle.personal_sync_dir.join(alice_file),
        b"alice body",
    )
    .expect("write alice file");

    // Each file must appear in its owner's space...
    poll_until(
        || async { admin_ocis.exists(admin_file).await.unwrap_or(false) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("admin file did not up-sync to admin space");
    poll_until(
        || async { alice_ocis.exists(alice_file).await.unwrap_or(false) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("alice file did not up-sync to Alice space");

    // ...and must NOT appear in the other account's space (no cross-contamination).
    assert!(
        !admin_ocis.exists(alice_file).await.unwrap_or(true),
        "Alice's file must not appear in the admin space"
    );
    assert!(
        !alice_ocis.exists(admin_file).await.unwrap_or(true),
        "admin's file must not appear in the Alice space"
    );

    // Cleanup (best-effort).
    let _ = provisioner.delete_user(&alice.id).await;
}

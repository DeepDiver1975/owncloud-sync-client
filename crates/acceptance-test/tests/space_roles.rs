// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Migrated from owncloud/client test/gui/tst_spaces (editable scenarios).
//!
//! Requires Tier 2 space + role provisioning: the admin creates a project space
//! and assigns a role (Space Editor / Manager / Viewer / Secure Viewer) to a
//! freshly-provisioned user via the Graph API, then that user adds the space as
//! an account and syncs it.
//!
//! The sync client performs NO client-side role enforcement (confirmed in the
//! design doc): a viewer's local edit is uploaded and rejected by the server.
//! So read-only roles are verified by a NEGATIVE server-side assertion (the
//! locally-written file never appears in the space), not a client-side block.
//! Client-side read-only enforcement is recorded as a product gap.

use std::time::Duration;

use acceptance_test::fixture::TestEnvironment;
use acceptance_test::ocis_client::OcisClient;
use acceptance_test::poll::poll_until;
use acceptance_test::provision::{role_ids, RoleAssignment, SpaceProvisioner, UserProvisioner};
use acceptance_test::testutil::skip_if_no_acceptance;

/// Everything a role scenario needs after setup: the live environment (held to
/// keep the daemon/GUI alive), the user provisioner and user id for cleanup, an
/// admin `OcisClient` scoped to the project space for server-side assertions,
/// and the local sync dir of the user's project space.
struct RoleScenario {
    /// Held only to keep the daemon/GUI alive via `Drop` for the test's
    /// duration; never read directly. CI runs clippy with `-D warnings`, so the
    /// otherwise-unread field is explicitly allowed rather than left to warn.
    #[allow(dead_code)]
    env: TestEnvironment,
    user_provisioner: UserProvisioner,
    user_id: String,
    space_admin_ocis: OcisClient,
    user_sync_dir: std::path::PathBuf,
}

/// Shared setup: provision a uniquely-named user + project space, assign the
/// built-in role `role_id`, and (if the role is enabled) add the user's account
/// on that space. Returns `None` when the role is unavailable on this oCIS, so
/// the caller can skip-and-log.
async fn setup_role_scenario(role_label: &str, role_id: &str) -> Option<RoleScenario> {
    let mut env = TestEnvironment::start()
        .await
        .expect("TestEnvironment::start");

    let user_provisioner = UserProvisioner::new(env.ocis_url.clone())
        .await
        .expect("UserProvisioner::new");
    let space_provisioner = SpaceProvisioner::new(env.ocis_url.clone())
        .await
        .expect("SpaceProvisioner::new");

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let short = &suffix[..8];
    let user_name = format!("{role_label}-{short}");
    let space_name = format!("项目 {role_label} {short}"); // non-ASCII space name

    let user = user_provisioner
        .create_user(&user_name, "Secret123!", role_label)
        .await
        .expect("create role user");
    let space = space_provisioner
        .create_project_space(&space_name)
        .await
        .expect("create project space");

    // Assign the role; bail out (skip) if this oCIS lacks the role definition.
    let assignment = space_provisioner
        .assign_role(&space.id, &user.id, role_id)
        .await
        .expect("assign_role");
    if assignment == RoleAssignment::Unavailable {
        eprintln!(
            "SKIP: role {role_id} is unavailable in this oCIS config; \
             scenario {role_label} not run"
        );
        let _ = user_provisioner.delete_user(&user.id).await;
        return None;
    }

    // The role-holder adds the project space as an account and syncs it.
    let (handle, _) = env
        .add_account_on_space(&user.username, &user.password, &space.name)
        .await
        .expect("add_account_on_space");

    // Admin client scoped to the project space for server-side assertions.
    let space_admin_ocis =
        OcisClient::from_credentials_on_space(env.ocis_url.clone(), "admin", "admin", &space.id)
            .await
            .expect("space admin OcisClient");

    Some(RoleScenario {
        env,
        user_provisioner,
        user_id: user.id,
        space_admin_ocis,
        user_sync_dir: handle.personal_sync_dir,
    })
}

/// Editors can write: full bidirectional round-trip in the shared space.
#[tokio::test]
async fn test_space_editor_round_trip() {
    if skip_if_no_acceptance() {
        return;
    }
    let scenario = setup_role_scenario("editor", role_ids::SPACE_EDITOR)
        .await
        .expect("Space Editor role must exist in a stock oCIS");
    assert_round_trip(&scenario).await;
    let _ = scenario
        .user_provisioner
        .delete_user(&scenario.user_id)
        .await;
}

/// Managers can write too: same round-trip, confirming manager assignment.
#[tokio::test]
async fn test_space_manager_round_trip() {
    if skip_if_no_acceptance() {
        return;
    }
    let scenario = setup_role_scenario("manager", role_ids::MANAGER)
        .await
        .expect("Space Manager role must exist in a stock oCIS");
    assert_round_trip(&scenario).await;
    let _ = scenario
        .user_provisioner
        .delete_user(&scenario.user_id)
        .await;
}

/// Viewers are read-only: down-sync works, a local edit never reaches the server.
#[tokio::test]
async fn test_space_viewer_read_only() {
    if skip_if_no_acceptance() {
        return;
    }
    let scenario = setup_role_scenario("viewer", role_ids::SPACE_VIEWER)
        .await
        .expect("Space Viewer role must exist in a stock oCIS");
    assert_read_only(&scenario).await;
    let _ = scenario
        .user_provisioner
        .delete_user(&scenario.user_id)
        .await;
}

/// Secure Viewers are read-only as well; skip-and-log if the role is unavailable.
#[tokio::test]
async fn test_secure_viewer_read_only() {
    if skip_if_no_acceptance() {
        return;
    }
    let Some(scenario) = setup_role_scenario("secureviewer", role_ids::SECURE_VIEWER).await else {
        return; // role unavailable on this oCIS — logged in setup_role_scenario
    };
    assert_read_only(&scenario).await;
    let _ = scenario
        .user_provisioner
        .delete_user(&scenario.user_id)
        .await;
}

/// Bidirectional round-trip for write-capable roles: a server-created file
/// down-syncs locally, and a locally-created file up-syncs to the space.
async fn assert_round_trip(s: &RoleScenario) {
    // Down-sync: admin PUTs into the space → appears in the user's local dir.
    let down_file = "项目 down?file.txt";
    s.space_admin_ocis
        .put(down_file, b"server body")
        .await
        .expect("admin put into project space");
    poll_until(
        || {
            let p = s.user_sync_dir.join(down_file);
            async move { p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server file did not down-sync to the user's local space dir");

    // Up-sync: user writes locally → appears in the space server-side.
    let up_file = "项目 up<file>.txt";
    std::fs::write(s.user_sync_dir.join(up_file), b"local body").expect("write local up file");
    poll_until(
        || async { s.space_admin_ocis.exists(up_file).await.unwrap_or(false) },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("local file did not up-sync to the project space");
}

/// Read-only behaviour: a server-created file still down-syncs (proving the
/// account is genuinely connected to the space), but a locally-created file is
/// NEVER accepted by the server.
async fn assert_read_only(s: &RoleScenario) {
    // Down-sync still works for read-only roles.
    let down_file = "项目 ro down?file.txt";
    s.space_admin_ocis
        .put(down_file, b"server body")
        .await
        .expect("admin put into project space");
    poll_until(
        || {
            let p = s.user_sync_dir.join(down_file);
            async move { p.exists() }
        },
        Duration::from_secs(30),
        Duration::from_secs(1),
    )
    .await
    .expect("server file did not down-sync for read-only role");

    // Up-sync must be rejected: write locally, wait, assert it never appears.
    let up_file = "项目 ro up<file>.txt";
    std::fs::write(s.user_sync_dir.join(up_file), b"forbidden body").expect("write local up file");
    // Give the daemon ample time to attempt (and the server to reject) the upload.
    tokio::time::sleep(Duration::from_secs(15)).await;
    assert!(
        !s.space_admin_ocis.exists(up_file).await.unwrap_or(true),
        "a read-only role's local file must NOT appear in the project space"
    );
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! Account setup against an ownCloud Classic (oc10) server.
//!
//! Exercises the Classic backend end-to-end: OIDC discovery fails on oc10, the
//! daemon falls back to the static OAuth2 endpoints with a `localhost` callback
//! (the only host oc10's pre-seeded desktop client accepts), resolves identity
//! via OCS, and synthesizes a single personal space so folder setup succeeds.

use acceptance_test::fixture::TestEnvironment;
use acceptance_test::testutil::skip_if_no_oc10_acceptance;
use daemon::config::AppConfig;

#[tokio::test]
async fn test_oc10_account_setup() {
    if skip_if_no_oc10_acceptance() {
        return;
    }

    let mut env = TestEnvironment::start_oc10()
        .await
        .expect("failed to start oc10 TestEnvironment");

    let callback_title = env
        .add_account()
        .await
        .expect("oc10 account setup via OAuth2 failed");

    assert_eq!(
        callback_title, "Successfully signed in",
        "expected success page title after OAuth2 login"
    );

    // Assert: config persisted exactly 1 Classic account with a non-empty
    // user_id (the OCS id `admin`, which roots the legacy WebDAV path) and 1
    // synthetic personal-space folder.
    let config_path = env.config_dir.path().join("owncloud").join("owncloud.toml");
    let cfg = AppConfig::load(&config_path).expect("failed to load config after add_account");

    assert_eq!(cfg.account.len(), 1, "expected exactly 1 account in config");
    let account = &cfg.account[0];
    assert_eq!(
        account.server_type,
        ocis_client::ServerType::Classic,
        "account should be detected as Classic (oc10)"
    );
    assert_eq!(
        account.user_id, "admin",
        "oc10 user_id should be the OCS id used in the WebDAV path"
    );
    assert_eq!(
        account.folder.len(),
        1,
        "oc10 has a single synthetic space, so expected exactly 1 folder"
    );
    // The synthetic space is named after the account display name ("admin"),
    // which is also the local sync sub-folder.
    assert_eq!(account.folder[0].display_name, env.personal_space_name);
}

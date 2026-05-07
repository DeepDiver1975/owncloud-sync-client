use acceptance_test::fixture::TestEnvironment;
use atspi::Role;
use daemon::config::AppConfig;
use std::time::Duration;

#[tokio::test]
async fn test_account_setup() {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping test_account_setup: OCIS_ACCEPTANCE not set");
        return;
    }

    let mut env = TestEnvironment::start()
        .await
        .expect("failed to start TestEnvironment");

    env.add_account()
        .await
        .expect("account setup via OIDC failed");

    // Assert: config file has exactly 1 account with non-empty user_id and 1 folder.
    let config_path = env.config_dir.path().join("owncloud").join("owncloud.toml");
    let cfg = AppConfig::load(&config_path).expect("failed to load config after add_account");

    assert_eq!(cfg.account.len(), 1, "expected exactly 1 account in config");
    let account = &cfg.account[0];
    assert!(
        !account.user_id.is_empty(),
        "expected user_id to be non-empty"
    );
    assert_eq!(
        account.folder.len(),
        1,
        "expected exactly 1 folder in account"
    );

    // Assert: the account display name is visible in the GUI SyncStatus view.
    // This is the key assertion: it confirms the GUI actually updated, not just the config.
    // If this fails with "timed out", check the dump_tree() output in the error message
    // and update Role::Label to the role the AT-SPI bridge emits for Iced text() widgets.
    env.atspi
        .wait_for_widget(Role::Label, &account.display_name, Duration::from_secs(10))
        .await
        .expect("account display name not visible in SyncStatus view");
}

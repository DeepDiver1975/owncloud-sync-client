use acceptance_test::fixture::TestEnvironment;
use daemon::config::AppConfig;

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
}

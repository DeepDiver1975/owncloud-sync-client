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

    let callback_title = env
        .add_account()
        .await
        .expect("account setup via OIDC failed");

    assert_eq!(
        callback_title, "Successfully signed in",
        "expected success page title after OIDC login"
    );

    // Assert: config file has exactly 1 account with non-empty user_id and 1 folder.
    let config_path = env.config_dir.path().join("owncloud").join("owncloud.toml");
    let cfg = AppConfig::load(&config_path).expect("failed to load config after add_account");

    assert_eq!(cfg.account.len(), 1, "expected exactly 1 account in config");
    let account = &cfg.account[0];
    assert!(
        !account.user_id.is_empty(),
        "expected user_id to be non-empty"
    );
    assert!(
        !account.folder.is_empty(),
        "expected at least 1 folder in account after setup"
    );
    let personal = account.folder.iter().find(|f| f.display_name == "Personal");
    assert!(personal.is_some(), "expected a 'Personal' folder");
}

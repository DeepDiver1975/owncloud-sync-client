use acceptance_test::fixture::TestEnvironment;

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
}

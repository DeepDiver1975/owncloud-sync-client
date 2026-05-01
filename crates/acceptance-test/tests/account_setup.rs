use acceptance_test::fixture::TestEnvironment;
use acceptance_test::playwright::complete_oidc_login;
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};
use std::time::Duration;

#[tokio::test]
async fn test_account_setup() {
    // Skip if acceptance tests are not enabled
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping test_account_setup: OCIS_ACCEPTANCE not set");
        return;
    }

    let mut env = TestEnvironment::start()
        .await
        .expect("failed to start TestEnvironment");

    // Trigger account setup via daemon IPC (iced 0.13 has no AT-SPI2 widget tree support,
    // so we drive the daemon directly rather than automating the GUI).
    env.daemon_ipc
        .send(DaemonCommand::AddAccount {
            url: env.ocis_url.to_string(),
        })
        .await
        .expect("failed to send AddAccount command");

    // Wait for AccountAddStarted event
    let _started = env
        .daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountAddStarted { .. }),
            Duration::from_secs(15),
        )
        .await
        .expect("AccountAddStarted event not received");

    // Read the OIDC authorization URL from daemon stdout
    let auth_url = env
        .wait_for_oidc_url()
        .await
        .expect("failed to get OIDC auth URL from daemon stdout");

    // Extract the callback port from the redirect_uri query parameter
    let callback_port = auth_url
        .query_pairs()
        .find_map(|(k, v)| {
            if k == "redirect_uri" {
                url::Url::parse(&v).ok().and_then(|u| u.port())
            } else {
                None
            }
        })
        .expect("could not extract callback port from redirect_uri in auth URL");

    // Complete the OIDC login via headless browser
    complete_oidc_login(&auth_url, callback_port, "admin", "admin")
        .await
        .expect("Playwright OIDC login failed");

    // Wait for AccountStateChanged { state: "added" } event
    env.daemon_ipc
        .wait_for(
            |e| {
                matches!(
                    e,
                    DaemonEvent::AccountStateChanged { state, .. } if state == "added"
                )
            },
            Duration::from_secs(30),
        )
        .await
        .expect("did not receive AccountStateChanged { state: \"added\" } event");
}

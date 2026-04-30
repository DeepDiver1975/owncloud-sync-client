use acceptance_test::atspi_client::AtSpiClient;
use acceptance_test::fixture::TestEnvironment;
use acceptance_test::playwright::complete_oidc_login;
use atspi::Role;
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

    // Connect to AT-SPI2 accessibility bus
    let atspi = AtSpiClient::connect()
        .await
        .expect("failed to connect to AT-SPI2");

    // Wait for the GUI main window to appear (e.g. a Window or Frame role)
    let _window = atspi
        .wait_for_widget(Role::Window, "ownCloud", Duration::from_secs(30))
        .await
        .expect("GUI window did not appear within 30s");

    // Click the "Add Account" button
    let add_account_btn = atspi
        .wait_for_widget(Role::Button, "Add Account", Duration::from_secs(10))
        .await
        .expect("'Add Account' button did not appear");
    atspi
        .click(&add_account_btn)
        .await
        .expect("failed to click 'Add Account'");

    // Wait for the URL text input to appear, enter the oCIS server URL, click Connect
    let url_input = atspi
        .wait_for_widget(Role::Entry, "Server URL", Duration::from_secs(10))
        .await
        .expect("URL input did not appear");
    atspi
        .set_text(&url_input, env.ocis_url.as_str())
        .await
        .expect("failed to set server URL");

    let connect_btn = atspi
        .wait_for_widget(Role::Button, "Connect", Duration::from_secs(5))
        .await
        .expect("'Connect' button did not appear");
    atspi
        .click(&connect_btn)
        .await
        .expect("failed to click 'Connect'");

    // Send AddAccount command via IPC
    env.daemon_ipc
        .send(DaemonCommand::AddAccount {
            url: env.ocis_url.to_string(),
        })
        .await
        .expect("failed to send AddAccount command");

    // Wait for AccountAddStarted event
    let started = env
        .daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountAddStarted { .. }),
            Duration::from_secs(15),
        )
        .await
        .expect("did not receive AccountAddStarted event");
    assert!(
        matches!(started, DaemonEvent::AccountAddStarted { .. }),
        "unexpected event: {started:?}"
    );

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
    let state_changed = env
        .daemon_ipc
        .wait_for(
            |e| {
                matches!(
                    e,
                    DaemonEvent::AccountStateChanged { state, .. } if state == "added"
                )
            },
            Duration::from_secs(30),
        )
        .await;

    assert!(
        state_changed.is_some(),
        "did not receive AccountStateChanged {{ state: \"added\" }} event"
    );
}

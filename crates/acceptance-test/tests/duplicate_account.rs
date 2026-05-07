use acceptance_test::fixture::TestEnvironment;
use acceptance_test::playwright::complete_oidc_login;
use atspi::Role;
use daemon::gui_ipc::protocol::DaemonEvent;
use std::time::Duration;

#[tokio::test]
async fn test_duplicate_account_rejected() {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping test_duplicate_account_rejected: OCIS_ACCEPTANCE not set");
        return;
    }

    let mut env = TestEnvironment::start()
        .await
        .expect("failed to start TestEnvironment");

    // First account setup via GUI — must succeed.
    env.add_account()
        .await
        .expect("first account setup via OIDC failed");

    // Second attempt: drive through the GUI exactly as a user would.

    // Click "Add Account" in the nav sidebar.
    let add_btn = env
        .atspi
        .wait_for_widget(Role::Button, "+ Add Account", Duration::from_secs(10))
        .await
        .expect("Add Account nav button not found for second attempt");
    env.atspi
        .click(&add_btn)
        .await
        .expect("failed to click Add Account for second attempt");

    // Type the same server URL.
    let url_field = env
        .atspi
        .wait_for_widget(
            Role::Entry,
            "https://your.server.com",
            Duration::from_secs(5),
        )
        .await
        .expect("URL text input not found for second attempt");
    env.atspi
        .set_text(&url_field, env.ocis_url.as_str())
        .await
        .expect("failed to set server URL for second attempt");

    // Click "Connect →".
    let connect_btn = env
        .atspi
        .wait_for_widget(Role::Button, "Connect →", Duration::from_secs(5))
        .await
        .expect("Connect button not found for second attempt");
    env.atspi
        .click(&connect_btn)
        .await
        .expect("failed to click Connect for second attempt");

    // Wait for daemon to confirm a new OIDC flow started.
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountAddStarted { .. }),
            Duration::from_secs(15),
        )
        .await
        .expect("AccountAddStarted not received for second attempt");

    // Complete OIDC login with the same credentials (same user = duplicate).
    let auth_url = env
        .wait_for_oidc_url()
        .await
        .expect("OIDC_AUTH_URL not emitted for second attempt");

    let callback_port = auth_url
        .query_pairs()
        .find_map(|(k, v)| {
            if k == "redirect_uri" {
                url::Url::parse(&v).ok().and_then(|u| u.port())
            } else {
                None
            }
        })
        .expect("could not extract callback port from redirect_uri");

    complete_oidc_login(&auth_url, callback_port, "admin", "admin")
        .await
        .expect("Playwright OIDC login failed for second attempt");

    // Daemon must reject the duplicate.
    let event = env
        .daemon_ipc
        .wait_for(
            |e| {
                matches!(
                    e,
                    DaemonEvent::AccountAddFailed { .. } | DaemonEvent::AccountAddCompleted { .. }
                )
            },
            Duration::from_secs(30),
        )
        .await
        .expect("neither AccountAddFailed nor AccountAddCompleted received");

    assert!(
        matches!(event, DaemonEvent::AccountAddFailed { .. }),
        "expected AccountAddFailed for duplicate account, got: {event:?}"
    );

    // GUI must have returned to the AddAccount view (URL input visible again).
    // The URL field is found by its placeholder text even when it has content —
    // AT-SPI accessible names for text inputs come from the placeholder, not the value.
    env.atspi
        .wait_for_widget(
            Role::Entry,
            "https://your.server.com",
            Duration::from_secs(10),
        )
        .await
        .expect("URL input not visible after duplicate rejection — GUI did not return to AddAccount view");
}

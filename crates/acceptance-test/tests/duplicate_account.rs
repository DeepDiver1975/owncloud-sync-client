use acceptance_test::fixture::TestEnvironment;
use acceptance_test::playwright::complete_oidc_login;
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};
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

    // First account setup — must succeed.
    env.add_account()
        .await
        .expect("first account setup via OIDC failed");

    // Send a second AddAccount command for the same server.
    env.daemon_ipc
        .send(DaemonCommand::AddAccount {
            url: env.bare_url(),
        })
        .await
        .expect("failed to send second AddAccount");

    // Wait for AccountAddStarted so we know the daemon opened a new OIDC flow.
    env.daemon_ipc
        .wait_for(
            |e| matches!(e, DaemonEvent::AccountAddStarted { .. }),
            Duration::from_secs(15),
        )
        .await
        .expect("AccountAddStarted not received for second AddAccount");

    // Complete OIDC login again via Playwright (same credentials — same user).
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

    // Wait for either AccountAddFailed or AccountAddCompleted — whichever arrives first.
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
}

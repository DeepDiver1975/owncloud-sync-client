//! Acceptance test: verify the GUI launches and the daemon becomes ready,
//! confirming the tray initialisation path (icon load + subscription wiring) does not crash.

use acceptance_test::fixture::TestEnvironment;

#[tokio::test]
async fn gui_launches_and_daemon_becomes_ready() {
    if std::env::var("OCIS_ACCEPTANCE").is_err() {
        eprintln!("Skipping gui_launches_and_daemon_becomes_ready: OCIS_ACCEPTANCE not set");
        return;
    }

    // TestEnvironment::start() launches the daemon and the GUI binary and waits
    // for the IPC socket to appear. If the tray init crashes the process, start()
    // will fail or timeout here.
    let _env = TestEnvironment::start()
        .await
        .expect("GUI and daemon should start without crashing");
}

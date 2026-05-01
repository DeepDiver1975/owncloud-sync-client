// Integration tests for the gui update logic. Since the app logic has moved
// into gui-core's AppCore, these tests exercise AppCore directly via gui_core.

use gui_core::{Action, AppCore, FolderStatus, ViewKind};
use uuid::Uuid;

#[test]
fn navigate_to_changes_active_view() {
    let mut core = AppCore::new();
    core.apply(Action::NavigateTo(ViewKind::AddAccount {
        url_input: String::new(),
        error: None,
    }));
    assert!(matches!(
        core.view_model().active_view,
        ViewKind::AddAccount { .. }
    ));
}

#[test]
fn toggle_window_flips_visibility() {
    let mut core = AppCore::new();
    assert!(core.view_model().window_visible);
    core.apply(Action::ToggleWindow);
    assert!(!core.view_model().window_visible);
    core.apply(Action::ToggleWindow);
    assert!(core.view_model().window_visible);
}

#[test]
fn add_account_url_changed_updates_input() {
    let mut core = AppCore::new();
    core.apply(Action::NavigateTo(ViewKind::AddAccount {
        url_input: String::new(),
        error: None,
    }));
    core.apply(Action::AddAccountUrlChanged(
        "https://cloud.test".to_string(),
    ));
    if let ViewKind::AddAccount { url_input, .. } = &core.view_model().active_view {
        assert_eq!(url_input, "https://cloud.test");
    } else {
        panic!("expected AddAccount view");
    }
}

#[test]
fn add_account_submit_empty_url_sets_error() {
    let mut core = AppCore::new();
    core.apply(Action::NavigateTo(ViewKind::AddAccount {
        url_input: String::new(),
        error: None,
    }));
    core.apply(Action::AddAccountSubmit);
    if let ViewKind::AddAccount { error, .. } = &core.view_model().active_view {
        assert!(error.is_some());
    } else {
        panic!("expected AddAccount view");
    }
}

#[test]
fn navigate_to_account_settings_and_back() {
    let account_id = Uuid::new_v4();
    let mut core = AppCore::new();
    core.apply(Action::NavigateTo(ViewKind::AccountSettings(account_id)));
    assert!(matches!(
        core.view_model().active_view,
        ViewKind::AccountSettings(_)
    ));
    core.apply(Action::NavigateTo(ViewKind::SyncStatus));
    assert!(matches!(
        core.view_model().active_view,
        ViewKind::SyncStatus
    ));
}

#[test]
fn add_account_submit_with_url_and_disconnected_daemon_sets_error() {
    let mut core = AppCore::new();
    core.apply(Action::NavigateTo(ViewKind::AddAccount {
        url_input: "https://cloud.example.com".to_string(),
        error: None,
    }));
    core.apply(Action::AddAccountSubmit);
    if let ViewKind::AddAccount { error, .. } = &core.view_model().active_view {
        assert!(error.is_some(), "expected error when daemon disconnected");
    } else {
        panic!(
            "expected AddAccount view, got {:?}",
            core.view_model().active_view
        );
    }
}

#[test]
fn folder_status_paused_variant_accessible() {
    let _: FolderStatus = FolderStatus::Paused;
    let _: FolderStatus = FolderStatus::Idle;
}

use daemon::gui_ipc::protocol::DaemonEvent;
use gui::app::{update, App, Message};
use gui::model::{AccountView, FolderStatus, FolderView, View};
use uuid::Uuid;

fn make_app_with_folder(folder_id: Uuid) -> App {
    let mut app = App::default();
    app.accounts.push(AccountView {
        id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        display_name: "Test".to_string(),
        folders: vec![FolderView {
            id: folder_id,
            display_name: "Docs".to_string(),
            local_path: "/home/user/docs".to_string(),
            status: FolderStatus::Idle,
            progress: None,
            errors: vec![],
        }],
    });
    app
}

#[test]
fn navigate_to_changes_active_view() {
    let mut app = App::default();
    update(
        &mut app,
        Message::NavigateTo(View::AddAccount {
            url_input: String::new(),
            error: None,
        }),
    );
    assert!(matches!(app.active_view, View::AddAccount { .. }));
}

#[test]
fn toggle_window_flips_visibility() {
    let mut app = App::default();
    assert!(app.window_visible);
    update(&mut app, Message::ToggleWindow);
    assert!(!app.window_visible);
    update(&mut app, Message::ToggleWindow);
    assert!(app.window_visible);
}

#[test]
fn add_account_url_changed_updates_input() {
    let mut app = App::default();
    app.active_view = View::AddAccount {
        url_input: String::new(),
        error: None,
    };
    update(
        &mut app,
        Message::AddAccountUrlChanged("https://cloud.test".to_string()),
    );
    if let View::AddAccount { url_input, .. } = &app.active_view {
        assert_eq!(url_input, "https://cloud.test");
    } else {
        panic!("expected AddAccount view");
    }
}

#[test]
fn add_account_submit_empty_url_sets_error() {
    let mut app = App::default();
    app.active_view = View::AddAccount {
        url_input: String::new(),
        error: None,
    };
    update(&mut app, Message::AddAccountSubmit);
    if let View::AddAccount { error, .. } = &app.active_view {
        assert!(error.is_some());
    } else {
        panic!("expected AddAccount view");
    }
}

#[test]
fn daemon_event_sync_started_sets_syncing() {
    let folder_id = Uuid::new_v4();
    let mut app = make_app_with_folder(folder_id);
    update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::SyncStarted { folder_id }),
    );
    let folder = &app.accounts[0].folders[0];
    assert!(matches!(folder.status, FolderStatus::Syncing));
}

#[test]
fn daemon_event_sync_finished_no_errors_sets_idle() {
    let folder_id = Uuid::new_v4();
    let mut app = make_app_with_folder(folder_id);
    app.accounts[0].folders[0].status = FolderStatus::Syncing;
    update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::SyncFinished {
            folder_id,
            errors: vec![],
        }),
    );
    let folder = &app.accounts[0].folders[0];
    assert!(matches!(folder.status, FolderStatus::Idle));
}

#[test]
fn daemon_event_sync_finished_with_errors_sets_error_status() {
    let folder_id = Uuid::new_v4();
    let mut app = make_app_with_folder(folder_id);
    update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::SyncFinished {
            folder_id,
            errors: vec!["conflict".to_string()],
        }),
    );
    let folder = &app.accounts[0].folders[0];
    assert!(matches!(folder.status, FolderStatus::Error));
    assert_eq!(folder.errors, vec!["conflict"]);
}

#[test]
fn pause_folder_sets_paused_status() {
    let folder_id = Uuid::new_v4();
    let mut app = make_app_with_folder(folder_id);
    update(&mut app, Message::PauseFolder(folder_id));
    let folder = &app.accounts[0].folders[0];
    assert!(matches!(folder.status, FolderStatus::Paused));
}

#[test]
fn resume_folder_sets_idle_status() {
    let folder_id = Uuid::new_v4();
    let mut app = make_app_with_folder(folder_id);
    app.accounts[0].folders[0].status = FolderStatus::Paused;
    update(&mut app, Message::ResumeFolder(folder_id));
    let folder = &app.accounts[0].folders[0];
    assert!(matches!(folder.status, FolderStatus::Idle));
}

#[test]
fn remove_account_removes_from_accounts_and_navigates_home() {
    let account_id = Uuid::new_v4();
    let mut app = App::default();
    app.accounts.push(AccountView {
        id: account_id,
        url: "https://example.com".to_string(),
        display_name: "Test".to_string(),
        folders: vec![],
    });
    app.active_view = View::AccountSettings(account_id);
    update(&mut app, Message::RemoveAccount(account_id));
    assert!(app.accounts.is_empty());
    assert!(matches!(app.active_view, View::SyncStatus));
}

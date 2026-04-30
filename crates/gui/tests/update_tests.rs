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
    let _ = update(
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
    let _ = update(&mut app, Message::ToggleWindow);
    assert!(!app.window_visible);
    let _ = update(&mut app, Message::ToggleWindow);
    assert!(app.window_visible);
}

#[test]
fn add_account_url_changed_updates_input() {
    let mut app = App {
        active_view: View::AddAccount {
            url_input: String::new(),
            error: None,
        },
        ..App::default()
    };
    let _ = update(
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
    let mut app = App {
        active_view: View::AddAccount {
            url_input: String::new(),
            error: None,
        },
        ..App::default()
    };
    let _ = update(&mut app, Message::AddAccountSubmit);
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
    let _ = update(
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
    let _ = update(
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
    let _ = update(
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
    let _ = update(&mut app, Message::PauseFolder(folder_id));
    let folder = &app.accounts[0].folders[0];
    assert!(matches!(folder.status, FolderStatus::Paused));
}

#[test]
fn resume_folder_sets_idle_status() {
    let folder_id = Uuid::new_v4();
    let mut app = make_app_with_folder(folder_id);
    app.accounts[0].folders[0].status = FolderStatus::Paused;
    let _ = update(&mut app, Message::ResumeFolder(folder_id));
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
    let _ = update(&mut app, Message::RemoveAccount(account_id));
    assert!(app.accounts.is_empty());
    assert!(matches!(app.active_view, View::SyncStatus));
}

#[test]
fn add_account_submit_with_url_and_disconnected_daemon_sets_error() {
    use gui::app::{update, App, Message};
    use gui::model::View;

    // With default App, daemon is disconnected — send returns false.
    // Supply a URL to bypass the empty-URL guard; it will fail at send
    // and set the "not connected" error.
    let mut app = App {
        active_view: View::AddAccount {
            url_input: "https://cloud.example.com".to_string(),
            error: None,
        },
        ..App::default()
    };
    let _ = update(&mut app, Message::AddAccountSubmit);
    // Disconnected daemon → should stay on AddAccount with error
    if let View::AddAccount { error, .. } = &app.active_view {
        assert!(error.is_some(), "expected error when daemon disconnected");
    } else {
        panic!("expected AddAccount view, got {:?}", app.active_view);
    }
}

#[test]
fn account_add_started_updates_waiting_view_account_id() {
    use daemon::gui_ipc::protocol::DaemonEvent;
    use gui::app::{update, App, Message};
    use gui::model::View;
    use uuid::Uuid;

    let account_id = Uuid::new_v4();
    let mut app = App {
        active_view: View::AddAccountWaiting {
            account_id: Uuid::nil(),
            url_input: "https://cloud.example.com".to_string(),
        },
        ..App::default()
    };
    let _ = update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::AccountAddStarted { account_id }),
    );
    if let View::AddAccountWaiting {
        account_id: stored_id,
        ..
    } = &app.active_view
    {
        assert_eq!(*stored_id, account_id);
    } else {
        panic!("expected AddAccountWaiting view");
    }
}

#[test]
fn account_state_changed_added_navigates_to_sync_status() {
    use daemon::gui_ipc::protocol::DaemonEvent;
    use gui::app::{update, App, Message};
    use gui::model::View;
    use uuid::Uuid;

    let account_id = Uuid::new_v4();
    let mut app = App {
        active_view: View::AddAccountWaiting {
            account_id,
            url_input: "https://cloud.example.com".to_string(),
        },
        ..App::default()
    };
    let _ = update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::AccountStateChanged {
            account_id,
            state: "added".to_string(),
        }),
    );
    assert!(matches!(app.active_view, View::SyncStatus));
}

#[test]
fn account_add_failed_returns_to_add_account_with_error_and_url() {
    use daemon::gui_ipc::protocol::DaemonEvent;
    use gui::app::{update, App, Message};
    use gui::model::View;
    use uuid::Uuid;

    let account_id = Uuid::new_v4();
    let mut app = App {
        active_view: View::AddAccountWaiting {
            account_id,
            url_input: "https://cloud.example.com".to_string(),
        },
        ..App::default()
    };
    let _ = update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::AccountAddFailed {
            account_id,
            reason: "discovery failed".to_string(),
        }),
    );
    if let View::AddAccount { url_input, error } = &app.active_view {
        assert_eq!(url_input, "https://cloud.example.com");
        assert_eq!(error.as_deref(), Some("discovery failed"));
    } else {
        panic!("expected AddAccount view");
    }
}

#[test]
fn add_account_submit_with_url_navigates_to_waiting_when_connected() {
    use gui::app::{update, App, Message};
    use gui::daemon_conn::DaemonConnection;
    use gui::model::View;
    use uuid::Uuid;

    let (conn, _rx) = DaemonConnection::connected_for_test();
    let mut app = App {
        daemon: conn,
        active_view: View::AddAccount {
            url_input: "https://cloud.example.com".to_string(),
            error: None,
        },
        ..App::default()
    };
    let _ = update(&mut app, Message::AddAccountSubmit);
    if let View::AddAccountWaiting {
        account_id,
        url_input,
    } = &app.active_view
    {
        assert!(
            account_id.is_nil(),
            "account_id should be nil before daemon responds"
        );
        assert_eq!(url_input, "https://cloud.example.com");
    } else {
        panic!("expected AddAccountWaiting view, got {:?}", app.active_view);
    }
}

#[test]
fn account_state_changed_added_with_nil_id_navigates_to_sync_status() {
    use daemon::gui_ipc::protocol::DaemonEvent;
    use gui::app::{update, App, Message};
    use gui::model::View;
    use uuid::Uuid;

    let some_account_id = Uuid::new_v4();
    let mut app = App {
        active_view: View::AddAccountWaiting {
            account_id: Uuid::nil(),
            url_input: "https://cloud.example.com".to_string(),
        },
        ..App::default()
    };
    let _ = update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::AccountStateChanged {
            account_id: some_account_id,
            state: "added".to_string(),
        }),
    );
    assert!(matches!(app.active_view, View::SyncStatus));
}

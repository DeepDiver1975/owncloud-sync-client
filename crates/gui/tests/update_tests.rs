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
fn account_state_changed_added_is_ignored() {
    use daemon::gui_ipc::protocol::DaemonEvent;
    use gui::app::{update, App, Message};
    use gui::model::View;
    use uuid::Uuid;

    // The "added" state is no longer acted upon — AccountAddCompleted handles
    // the transition instead. The view should remain unchanged.
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
    assert!(matches!(app.active_view, View::AddAccountWaiting { .. }));
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
fn account_state_changed_added_with_nil_id_is_ignored() {
    use daemon::gui_ipc::protocol::DaemonEvent;
    use gui::app::{update, App, Message};
    use gui::model::View;
    use uuid::Uuid;

    // The "added" state is no longer acted upon — AccountAddCompleted handles
    // the transition instead. The view should remain unchanged.
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
    assert!(matches!(app.active_view, View::AddAccountWaiting { .. }));
}

// ── New tests for PickLocalFolder flow ────────────────────────────────────────

#[test]
fn account_add_completed_adds_account_and_navigates_to_pick_folder() {
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
        Message::DaemonEvent(DaemonEvent::AccountAddCompleted {
            account_id,
            user_id: "alice".to_string(),
            display_name: "Alice Smith".to_string(),
            url: "https://cloud.example.com".to_string(),
        }),
    );
    assert_eq!(app.accounts.len(), 1);
    assert_eq!(app.accounts[0].id, account_id);
    assert!(
        matches!(&app.active_view, View::PickLocalFolder { account_id: aid, .. } if *aid == account_id)
    );
}

#[test]
fn pick_local_folder_path_changed_updates_input() {
    use gui::app::{update, App, Message};
    use gui::model::View;
    use uuid::Uuid;

    let account_id = Uuid::new_v4();
    let mut app = App {
        active_view: View::PickLocalFolder {
            account_id,
            display_name: "Alice".to_string(),
            url: "https://cloud.example.com".to_string(),
            local_path_input: String::new(),
            error: None,
        },
        ..App::default()
    };
    let _ = update(
        &mut app,
        Message::PickLocalFolderPathChanged("/some/path".to_string()),
    );
    if let View::PickLocalFolder {
        local_path_input, ..
    } = &app.active_view
    {
        assert_eq!(local_path_input, "/some/path");
    } else {
        panic!("expected PickLocalFolder view");
    }
}

#[test]
fn pick_local_folder_submit_empty_path_sets_error() {
    use gui::app::{update, App, Message};
    use gui::model::View;
    use uuid::Uuid;

    let account_id = Uuid::new_v4();
    let mut app = App {
        active_view: View::PickLocalFolder {
            account_id,
            display_name: "Alice".to_string(),
            url: "https://cloud.example.com".to_string(),
            local_path_input: String::new(),
            error: None,
        },
        ..App::default()
    };
    let _ = update(&mut app, Message::PickLocalFolderSubmit);
    if let View::PickLocalFolder { error, .. } = &app.active_view {
        assert!(error.is_some(), "expected error for empty path");
    } else {
        panic!("expected PickLocalFolder view");
    }
}

#[test]
fn pick_local_folder_submit_valid_path_sends_command() {
    use daemon::gui_ipc::protocol::DaemonCommand;
    use gui::app::{update, App, Message};
    use gui::daemon_conn::DaemonConnection;
    use gui::model::View;
    use uuid::Uuid;

    let (conn, mut rx) = DaemonConnection::connected_for_test();
    let account_id = Uuid::new_v4();
    let mut app = App {
        daemon: conn,
        active_view: View::PickLocalFolder {
            account_id,
            display_name: "Alice".to_string(),
            url: "https://cloud.example.com".to_string(),
            local_path_input: "/home/alice/owncloud".to_string(),
            error: None,
        },
        ..App::default()
    };
    let _ = update(&mut app, Message::PickLocalFolderSubmit);
    let cmd = rx.try_recv().expect("expected a command to be sent");
    assert!(
        matches!(cmd, DaemonCommand::SetAccountFolder { account_id: aid, local_path: ref p }
            if aid == account_id && p == "/home/alice/owncloud"),
        "unexpected command: {cmd:?}"
    );
}

#[test]
fn account_folder_added_adds_folder_and_navigates_to_sync_status() {
    use daemon::gui_ipc::protocol::DaemonEvent;
    use gui::app::{update, App, Message};
    use gui::model::{AccountView, View};
    use uuid::Uuid;

    let account_id = Uuid::new_v4();
    let folder_id = Uuid::new_v4();
    let mut app = App {
        active_view: View::PickLocalFolder {
            account_id,
            display_name: "Alice".to_string(),
            url: "https://cloud.example.com".to_string(),
            local_path_input: "/home/alice/owncloud".to_string(),
            error: None,
        },
        ..App::default()
    };
    app.accounts.push(AccountView {
        id: account_id,
        url: "https://cloud.example.com".to_string(),
        display_name: "Alice".to_string(),
        folders: vec![],
    });
    let _ = update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::AccountFolderAdded {
            account_id,
            folder_id,
            local_path: "/home/alice/owncloud".to_string(),
            display_name: "owncloud".to_string(),
        }),
    );
    assert_eq!(app.accounts[0].folders.len(), 1);
    assert_eq!(app.accounts[0].folders[0].id, folder_id);
    assert!(matches!(app.active_view, View::SyncStatus));
}

#[test]
fn account_set_folder_failed_sets_inline_error() {
    use daemon::gui_ipc::protocol::DaemonEvent;
    use gui::app::{update, App, Message};
    use gui::model::View;
    use uuid::Uuid;

    let account_id = Uuid::new_v4();
    let mut app = App {
        active_view: View::PickLocalFolder {
            account_id,
            display_name: "Alice".to_string(),
            url: "https://cloud.example.com".to_string(),
            local_path_input: "/home/alice/owncloud".to_string(),
            error: None,
        },
        ..App::default()
    };
    let _ = update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::AccountSetFolderFailed {
            account_id,
            reason: "path does not exist".to_string(),
        }),
    );
    if let View::PickLocalFolder { error, .. } = &app.active_view {
        assert_eq!(error.as_deref(), Some("path does not exist"));
    } else {
        panic!("expected PickLocalFolder view");
    }
}

#[test]
fn daemon_disconnected_does_not_reach_app_update() {
    // DaemonDisconnected is intercepted by IcedApp::update before reaching
    // the shared `update()` function. Calling `update()` directly with
    // DaemonDisconnected should therefore be a no-op (the function has no
    // arm for it — it falls through to the implicit unit return).
    // We verify the app state is unchanged as a proxy for this.
    let mut app = App::default();
    let _task = update(&mut app, Message::DaemonDisconnected);
    // App state must be untouched — view stays SyncStatus, no accounts added.
    assert!(matches!(app.active_view, View::SyncStatus));
    assert!(app.accounts.is_empty());
}

#[test]
fn pick_local_folder_cancel_sends_remove_account_and_navigates() {
    use daemon::gui_ipc::protocol::DaemonCommand;
    use gui::app::{update, App, Message};
    use gui::daemon_conn::DaemonConnection;
    use gui::model::View;
    use uuid::Uuid;

    let (conn, mut rx) = DaemonConnection::connected_for_test();
    let account_id = Uuid::new_v4();
    let mut app = App {
        daemon: conn,
        active_view: View::PickLocalFolder {
            account_id,
            display_name: "Alice".to_string(),
            url: "https://cloud.example.com".to_string(),
            local_path_input: String::new(),
            error: None,
        },
        ..App::default()
    };
    let _ = update(&mut app, Message::PickLocalFolderCancel);
    let cmd = rx.try_recv().expect("expected a command to be sent");
    assert!(
        matches!(cmd, DaemonCommand::RemoveAccount { account_id: aid } if aid == account_id),
        "unexpected command: {cmd:?}"
    );
    assert!(matches!(app.active_view, View::SyncStatus));
}

#[test]
fn navigate_to_folder_errors_view() {
    let account_id = Uuid::new_v4();
    let folder_id = Uuid::new_v4();
    let mut app = App::default();
    app.accounts.push(AccountView {
        id: account_id,
        url: "https://example.com".to_string(),
        display_name: "Test".to_string(),
        folders: vec![FolderView {
            id: folder_id,
            display_name: "Docs".to_string(),
            local_path: "/home/user/docs".to_string(),
            status: FolderStatus::Error,
            progress: None,
            errors: vec!["HTTP 503: unavailable".to_string()],
        }],
    });
    let _ = update(
        &mut app,
        Message::NavigateTo(gui::model::View::FolderErrors {
            account_id,
            folder_id,
        }),
    );
    assert!(matches!(
        app.active_view,
        gui::model::View::FolderErrors { .. }
    ));
}

fn account_snapshot_populates_accounts_with_correct_status_mapping() {
    use daemon::gui_ipc::protocol::{AccountSnapshot, DaemonEvent, FolderSnapshot};
    use gui::app::{update, App, Message};
    use gui::model::{FolderStatus, View};
    use uuid::Uuid;

    let account_id = Uuid::new_v4();
    let folder_idle_id = Uuid::new_v4();
    let folder_syncing_id = Uuid::new_v4();
    let folder_paused_id = Uuid::new_v4();

    let mut app = App::default();
    // Pre-populate with a stale account to verify it gets replaced.
    app.accounts.push(gui::model::AccountView {
        id: Uuid::new_v4(),
        url: "https://old.example.com".to_string(),
        display_name: "Old Account".to_string(),
        folders: vec![],
    });

    let _ = update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::AccountSnapshot {
            accounts: vec![AccountSnapshot {
                account_id,
                url: "https://ocis.example.com".to_string(),
                display_name: "Alice".to_string(),
                folders: vec![
                    FolderSnapshot {
                        folder_id: folder_idle_id,
                        display_name: "Personal".to_string(),
                        local_path: "/home/alice/ownCloud".to_string(),
                        status: "idle".to_string(),
                    },
                    FolderSnapshot {
                        folder_id: folder_syncing_id,
                        display_name: "Shared".to_string(),
                        local_path: "/home/alice/shared".to_string(),
                        status: "syncing".to_string(),
                    },
                    FolderSnapshot {
                        folder_id: folder_paused_id,
                        display_name: "Archive".to_string(),
                        local_path: "/home/alice/archive".to_string(),
                        status: "paused".to_string(),
                    },
                ],
            }],
        }),
    );

    assert_eq!(
        app.accounts.len(),
        1,
        "snapshot should replace all existing accounts"
    );
    let account = &app.accounts[0];
    assert_eq!(account.id, account_id);
    assert_eq!(account.url, "https://ocis.example.com");
    assert_eq!(account.display_name, "Alice");
    assert_eq!(account.folders.len(), 3);

    let f0 = &account.folders[0];
    assert_eq!(f0.id, folder_idle_id);
    assert!(
        matches!(f0.status, FolderStatus::Idle),
        "idle string should map to Idle"
    );

    let f1 = &account.folders[1];
    assert_eq!(f1.id, folder_syncing_id);
    assert!(
        matches!(f1.status, FolderStatus::Syncing),
        "syncing string should map to Syncing"
    );

    let f2 = &account.folders[2];
    assert_eq!(f2.id, folder_paused_id);
    assert!(
        matches!(f2.status, FolderStatus::Paused),
        "paused string should map to Paused"
    );

    // View should remain unchanged (SyncStatus).
    assert!(matches!(app.active_view, View::SyncStatus));
}

#[cfg(not(feature = "tray-icon"))]
#[test]
fn tray_handle_noop_build_succeeds() {
    let handle = gui::tray::TrayHandle::build().expect("no-op tray build should succeed");
    let _sub = handle.tray_events(); // must compile and not panic
}

#[cfg(not(feature = "tray-icon"))]
#[test]
fn tray_subscription_is_merged_compile_check() {
    let handle = gui::tray::TrayHandle::build().unwrap();
    let _: iced::Subscription<gui::app::Message> = handle.tray_events();
}


#[test]
fn account_snapshot_unknown_status_defaults_to_idle() {
    use daemon::gui_ipc::protocol::{AccountSnapshot, DaemonEvent, FolderSnapshot};
    use gui::app::{update, App, Message};
    use gui::model::FolderStatus;
    use uuid::Uuid;

    let mut app = App::default();
    let _ = update(
        &mut app,
        Message::DaemonEvent(DaemonEvent::AccountSnapshot {
            accounts: vec![AccountSnapshot {
                account_id: Uuid::new_v4(),
                url: "https://ocis.example.com".to_string(),
                display_name: "Bob".to_string(),
                folders: vec![FolderSnapshot {
                    folder_id: Uuid::new_v4(),
                    display_name: "Docs".to_string(),
                    local_path: "/home/bob/docs".to_string(),
                    status: "unknown-future-status".to_string(),
                }],
            }],
        }),
    );

    assert_eq!(app.accounts.len(), 1);
    assert!(matches!(
        app.accounts[0].folders[0].status,
        FolderStatus::Idle
    ));
}

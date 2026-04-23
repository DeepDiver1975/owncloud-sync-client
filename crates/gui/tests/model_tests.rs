use gui::app::App;
use gui::model::{AccountView, FolderStatus, FolderView, View};
use uuid::Uuid;

#[test]
fn app_default_has_empty_accounts() {
    let app = App::default();
    assert!(app.accounts.is_empty());
}

#[test]
fn app_default_active_view_is_sync_status() {
    let app = App::default();
    assert!(matches!(app.active_view, View::SyncStatus));
}

#[test]
fn app_default_window_visible() {
    let app = App::default();
    assert!(app.window_visible);
}

#[test]
fn folder_status_variants_exist() {
    let _: FolderStatus = FolderStatus::Idle;
    let _: FolderStatus = FolderStatus::Syncing;
    let _: FolderStatus = FolderStatus::Error;
    let _: FolderStatus = FolderStatus::Paused;
}

#[test]
fn folder_view_construction() {
    let id = Uuid::new_v4();
    let folder = FolderView {
        id,
        display_name: "My Docs".to_string(),
        local_path: "/home/user/docs".to_string(),
        status: FolderStatus::Idle,
        progress: None,
        errors: vec![],
    };
    assert_eq!(folder.id, id);
    assert_eq!(folder.display_name, "My Docs");
    assert_eq!(folder.local_path, "/home/user/docs");
    assert!(matches!(folder.status, FolderStatus::Idle));
    assert!(folder.progress.is_none());
    assert!(folder.errors.is_empty());
}

#[test]
fn account_view_construction() {
    let id = Uuid::new_v4();
    let account = AccountView {
        id,
        url: "https://cloud.example.com".to_string(),
        display_name: "Example Cloud".to_string(),
        folders: vec![],
    };
    assert_eq!(account.id, id);
    assert_eq!(account.url, "https://cloud.example.com");
    assert!(account.folders.is_empty());
}

#[test]
fn view_add_account_holds_input_state() {
    let view = View::AddAccount {
        url_input: "https://my.server.org".to_string(),
        error: Some("Server not found".to_string()),
    };
    if let View::AddAccount { url_input, error } = view {
        assert_eq!(url_input, "https://my.server.org");
        assert_eq!(error.as_deref(), Some("Server not found"));
    } else {
        panic!("expected View::AddAccount");
    }
}

#[test]
fn folder_status_debug_clone() {
    let s = FolderStatus::Syncing;
    let s2 = s.clone();
    assert!(format!("{s2:?}").contains("Syncing"));
}

#[test]
fn folder_status_display() {
    assert_eq!(FolderStatus::Idle.to_string(), "Idle");
    assert_eq!(FolderStatus::Syncing.to_string(), "Syncing");
    assert_eq!(FolderStatus::Error.to_string(), "Error");
    assert_eq!(FolderStatus::Paused.to_string(), "Paused");
}

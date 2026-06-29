// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

use std::collections::HashMap;
use std::sync::Arc;
use tempfile::{NamedTempFile, TempDir};
use tokio::sync::Mutex;
use uuid::Uuid;

use daemon::config::{AccountConfig, AppConfig, GeneralConfig};
use daemon::folder_manager::FolderManager;
use daemon::gui_ipc::handler::{handle_command, HandleContext, ShouldQuit};
use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};
use daemon::gui_ipc::GuiIpcServer;
use daemon::scheduler::SyncScheduler;
use ocis_client::auth::oidc::TokenSet;
use ocis_client::auth::{OidcAuth, TokenManager};
use ocis_client::ServerType;
use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_ctx<'a>(
    ipc: Arc<GuiIpcServer>,
    scheduler: Arc<Mutex<SyncScheduler>>,
    config: Arc<Mutex<AppConfig>>,
    file: &NamedTempFile,
    fm: &'a mut FolderManager,
    watcher_tx: tokio::sync::mpsc::Sender<(Uuid, notify::Event)>,
) -> HandleContext<'a> {
    HandleContext {
        scheduler,
        folder_manager: fm,
        ipc,
        config,
        config_path: file.path().to_path_buf(),
        live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
        token_managers: Arc::new(std::sync::RwLock::new(
            HashMap::<Uuid, Arc<TokenManager>>::new(),
        )),
        watcher_tx,
    }
}

#[tokio::test]
async fn list_spaces_unknown_account_broadcasts_failed() {
    let (ipc, mut rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();
    let (tx, _rx) = tokio::sync::mpsc::channel(1);

    let result = handle_command(
        DaemonCommand::ListSpaces {
            account_id: Uuid::new_v4(),
        },
        &mut make_ctx(ipc, scheduler, config, &file, &mut fm, tx),
    )
    .await
    .unwrap();

    assert_eq!(result, ShouldQuit::No);
    let event = rx.try_recv().expect("expected event");
    assert!(matches!(event, DaemonEvent::AccountSpaceFailed { .. }));
}

#[tokio::test]
async fn dismiss_space_adds_to_dismissed_list() {
    let (ipc, _rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let account_id = Uuid::new_v4();
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: account_id,
            url: "https://ocis.example.com".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            server_type: ServerType::Ocis,
            folder: vec![],
            dismissed_spaces: vec![],
        }],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();
    let (tx, _rx) = tokio::sync::mpsc::channel(1);

    handle_command(
        DaemonCommand::DismissSpace {
            account_id,
            space_id: "space-abc".into(),
        },
        &mut make_ctx(ipc, scheduler, config.clone(), &file, &mut fm, tx),
    )
    .await
    .unwrap();

    let cfg = config.lock().await;
    let acc = cfg.account.iter().find(|a| a.id == account_id).unwrap();
    assert!(acc.dismissed_spaces.contains(&"space-abc".to_string()));
}

#[tokio::test]
async fn set_account_folders_unknown_account_broadcasts_failed() {
    let (ipc, mut rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();
    let (tx, _rx) = tokio::sync::mpsc::channel(1);

    handle_command(
        DaemonCommand::SetAccountFolders {
            account_id: Uuid::new_v4(),
            root_path: "/tmp".into(),
            spaces: vec![],
        },
        &mut make_ctx(ipc, scheduler, config, &file, &mut fm, tx),
    )
    .await
    .unwrap();

    let event = rx.try_recv().expect("expected event");
    assert!(matches!(event, DaemonEvent::AccountSpaceFailed { .. }));
}

#[tokio::test]
async fn set_account_folders_no_token_manager_broadcasts_failed() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("ownCloud");
    std::fs::create_dir_all(&root).unwrap();

    let (ipc, mut rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let account_id = Uuid::new_v4();
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: account_id,
            url: "https://ocis.example.com".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            server_type: ServerType::Ocis,
            folder: vec![],
            dismissed_spaces: vec![],
        }],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();
    let (tx, _rx) = tokio::sync::mpsc::channel(256);

    handle_command(
        DaemonCommand::SetAccountFolders {
            account_id,
            root_path: root.to_string_lossy().into_owned(),
            spaces: vec![daemon::gui_ipc::protocol::SpaceSelection {
                space_id: "personal-space-id".into(),
                display_name: "Personal".into(),
            }],
        },
        &mut make_ctx(ipc, scheduler, config, &file, &mut fm, tx),
    )
    .await
    .unwrap();

    let event = rx.try_recv().expect("expected event");
    assert!(
        matches!(event, DaemonEvent::AccountSpaceFailed { .. }),
        "expected AccountSpaceFailed, got {event:?}"
    );
}

/// Builds a usable `TokenManager` backed by a wiremock OIDC discovery endpoint,
/// mirroring `folder_manager`'s in-crate test helper. The token never expires,
/// so no real token endpoint is hit.
async fn make_token_manager(account_id: Uuid) -> (MockServer, Arc<TokenManager>) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"{{
                "issuer": "{uri}",
                "authorization_endpoint": "{uri}/auth",
                "token_endpoint": "{uri}/token"
            }}"#,
            uri = server.uri()
        )))
        .mount(&server)
        .await;

    let oidc = OidcAuth::discover(
        &server.uri(),
        "test-client",
        None,
        "http://localhost:9999/callback",
        false,
    )
    .await
    .unwrap();

    let token = TokenSet {
        access_token: "test".into(),
        refresh_token: None,
        expires_at: i64::MAX,
    };
    let tm = Arc::new(TokenManager::new(oidc, token, account_id.to_string()));
    (server, tm)
}

/// Regression test for GitHub issue #15 requirement 2 ("not-existing local
/// folder needs to be created"). Sends `SetAccountFolders` with a `root_path`
/// that does NOT yet exist and asserts the daemon creates the directory on
/// disk. `create_dir_all` on `root/<display_name>` creates the missing root as
/// an intermediate dir, so a brand-new root is materialized before any sync.
///
/// Note: this asserts only the directory-creation behavior (the precise gap in
/// issue #15). The subsequent engine registration (`add_folder`) opens a sync
/// journal under the real platform config dir and may fail in an isolated test
/// environment; that side of the flow is out of scope here and exercised by the
/// acceptance suite. Directory creation happens *before* engine registration,
/// so it is observable regardless.
#[tokio::test]
async fn set_account_folders_creates_nonexistent_root() {
    let dir = TempDir::new().unwrap();
    // Root path that does NOT exist yet (two missing levels under the temp dir).
    let root = dir.path().join("does-not-exist-yet").join("ownCloud");
    assert!(
        !root.exists(),
        "precondition: root must not exist before the command"
    );

    let account_id = Uuid::new_v4();
    let (_server, tm) = make_token_manager(account_id).await;

    let (ipc, mut rx) = GuiIpcServer::new();
    let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: account_id,
            url: "https://ocis.example.com".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            server_type: ServerType::Ocis,
            folder: vec![],
            dismissed_spaces: vec![],
        }],
    }));
    let file = NamedTempFile::new().unwrap();
    let mut fm = FolderManager::empty();
    let (watcher_tx, _watcher_rx) = tokio::sync::mpsc::channel(256);

    let mut token_managers = HashMap::<Uuid, Arc<TokenManager>>::new();
    token_managers.insert(account_id, tm);

    let space_name = "Personal";
    handle_command(
        DaemonCommand::SetAccountFolders {
            account_id,
            root_path: root.to_string_lossy().into_owned(),
            spaces: vec![daemon::gui_ipc::protocol::SpaceSelection {
                space_id: "personal-space-id".into(),
                display_name: space_name.into(),
            }],
        },
        &mut HandleContext {
            scheduler,
            folder_manager: &mut fm,
            ipc,
            config,
            config_path: file.path().to_path_buf(),
            live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
            token_managers: Arc::new(std::sync::RwLock::new(token_managers)),
            watcher_tx,
        },
    )
    .await
    .unwrap();

    // The previously-missing root and the per-space subdir now exist on disk.
    let space_dir = root.join(space_name);
    assert!(
        std::fs::metadata(&space_dir)
            .map(|m| m.is_dir())
            .unwrap_or(false),
        "expected daemon to create {} (incl. the non-existent root)",
        space_dir.display()
    );
    assert!(
        root.is_dir(),
        "the non-existent root itself must be created"
    );

    // Drain events so the channel isn't dropped early; this is the directory-
    // creation regression, so we do not assert on engine-registration outcome.
    while rx.try_recv().is_ok() {}
}

/// Security regression: a server-controlled space `display_name` that is NOT a
/// single safe path segment (`../escape`, `/etc/...`, etc.) must be rejected
/// with `AccountSpaceFailed` and must NOT cause any directory to be created
/// outside the chosen root. `Path::join` discards the base when the component
/// is absolute, and `..` is resolved by the OS — so without validation a
/// malicious/compromised server could escape `root_path` (path traversal ->
/// arbitrary directory creation, and the escaped path would then be persisted
/// as the sync engine's local_root).
#[tokio::test]
async fn set_account_folders_rejects_unsafe_space_name() {
    for unsafe_name in ["../escape", "/etc/ocsync-escape-test"] {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("ownCloud");

        let account_id = Uuid::new_v4();
        let (_server, tm) = make_token_manager(account_id).await;

        let (ipc, mut rx) = GuiIpcServer::new();
        let scheduler = Arc::new(Mutex::new(SyncScheduler::new(vec![])));
        let config = Arc::new(Mutex::new(AppConfig {
            general: GeneralConfig::default(),
            account: vec![AccountConfig {
                id: account_id,
                url: "https://ocis.example.com".into(),
                user_id: "alice".into(),
                username: "alice".into(),
                display_name: "Alice".into(),
                server_type: ServerType::Ocis,
                folder: vec![],
                dismissed_spaces: vec![],
            }],
        }));
        let file = NamedTempFile::new().unwrap();
        let mut fm = FolderManager::empty();
        let (watcher_tx, _watcher_rx) = tokio::sync::mpsc::channel(256);

        let mut token_managers = HashMap::<Uuid, Arc<TokenManager>>::new();
        token_managers.insert(account_id, tm);

        handle_command(
            DaemonCommand::SetAccountFolders {
                account_id,
                root_path: root.to_string_lossy().into_owned(),
                spaces: vec![daemon::gui_ipc::protocol::SpaceSelection {
                    space_id: "personal-space-id".into(),
                    display_name: unsafe_name.into(),
                }],
            },
            &mut HandleContext {
                scheduler,
                folder_manager: &mut fm,
                ipc,
                config,
                config_path: file.path().to_path_buf(),
                live_folder_ids: Arc::new(std::sync::RwLock::new(vec![])),
                token_managers: Arc::new(std::sync::RwLock::new(token_managers)),
                watcher_tx,
            },
        )
        .await
        .unwrap();

        // The unsafe name must be rejected: AccountSpaceFailed, no AccountFolderAdded.
        let mut saw_failed = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                DaemonEvent::AccountSpaceFailed { .. } => saw_failed = true,
                DaemonEvent::AccountFolderAdded { .. } => {
                    panic!("unsafe name {unsafe_name:?} must NOT be accepted")
                }
                _ => {}
            }
        }
        assert!(
            saw_failed,
            "expected AccountSpaceFailed for unsafe name {unsafe_name:?}"
        );

        // Nothing must be created outside the chosen root.
        let escaped = std::path::Path::new(&root.to_string_lossy().into_owned()).join(unsafe_name);
        assert!(
            !escaped.exists(),
            "escape path {} must not be created",
            escaped.display()
        );
        // The absolute-path case targets a real system dir; assert that the
        // specific escape target was not created by us. (`/etc` itself exists,
        // so we check the leaf, which our temp-isolated test never legitimately
        // creates.)
        if unsafe_name.starts_with('/') {
            assert!(
                !std::path::Path::new(unsafe_name).exists(),
                "absolute escape target {unsafe_name} must not be created"
            );
        }
    }
}

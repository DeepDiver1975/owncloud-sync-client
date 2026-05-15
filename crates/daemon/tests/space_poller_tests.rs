use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use daemon::config::{AccountConfig, AppConfig, FolderConfig, GeneralConfig};
use daemon::gui_ipc::protocol::DaemonEvent;
use daemon::gui_ipc::GuiIpcServer;
use daemon::space_poller::SpacePoller;
use ocis_client::auth::oidc::TokenSet;
use ocis_client::auth::OidcAuth;
use ocis_client::auth::TokenManager;

async fn make_server_with_spaces(spaces_json: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"{{"issuer":"{uri}","authorization_endpoint":"{uri}/auth","token_endpoint":"{uri}/token"}}"#,
            uri = server.uri()
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/graph/v1.0/me/drives"))
        .respond_with(ResponseTemplate::new(200).set_body_string(spaces_json.to_string()))
        .mount(&server)
        .await;
    server
}

async fn make_tm(server: &MockServer, account_id: Uuid) -> Arc<TokenManager> {
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
    Arc::new(TokenManager::new(oidc, token, account_id.to_string()))
}

#[tokio::test]
async fn poller_emits_space_discovered_for_new_space() {
    let account_id = Uuid::new_v4();
    let server = make_server_with_spaces(
        r#"{"value":[{"id":"new-space","name":"ProjectX","driveType":"project","webUrl":"","quota":null}]}"#,
    )
    .await;
    let tm = make_tm(&server, account_id).await;

    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: account_id,
            url: server.uri(),
            user_id: "alice".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            folder: vec![],
            dismissed_spaces: vec![],
        }],
    }));

    let (ipc, mut rx) = GuiIpcServer::new();
    let cancel = CancellationToken::new();

    let poller = SpacePoller::new(
        account_id,
        Arc::clone(&config),
        Arc::new(std::path::PathBuf::from("/tmp/test.toml")),
        Arc::clone(&ipc),
        Arc::clone(&tm),
        Duration::from_millis(50),
        cancel.clone(),
    );

    let handle = tokio::spawn(async move { poller.run().await });

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let discovered = loop {
        if let Ok(evt) = rx.try_recv() {
            if matches!(evt, DaemonEvent::SpaceDiscovered { .. }) {
                break evt;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("SpaceDiscovered not received within 3s");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    cancel.cancel();
    let _ = handle.await;

    match discovered {
        DaemonEvent::SpaceDiscovered {
            space_id,
            space_name,
            ..
        } => {
            assert_eq!(space_id, "new-space");
            assert_eq!(space_name, "ProjectX");
        }
        _ => panic!("wrong event"),
    }
}

#[tokio::test]
async fn poller_does_not_re_emit_dismissed_space() {
    let account_id = Uuid::new_v4();
    let server = make_server_with_spaces(
        r#"{"value":[{"id":"dismissed-space","name":"Old","driveType":"project","webUrl":"","quota":null}]}"#,
    )
    .await;
    let tm = make_tm(&server, account_id).await;

    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: account_id,
            url: server.uri(),
            user_id: "alice".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            folder: vec![],
            dismissed_spaces: vec!["dismissed-space".to_string()],
        }],
    }));

    let (ipc, mut rx) = GuiIpcServer::new();
    let cancel = CancellationToken::new();
    let poller = SpacePoller::new(
        account_id,
        config,
        Arc::new(std::path::PathBuf::from("/tmp/test.toml")),
        Arc::clone(&ipc),
        tm,
        Duration::from_millis(50),
        cancel.clone(),
    );

    tokio::spawn(async move { poller.run().await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    cancel.cancel();

    let mut count = 0;
    while let Ok(evt) = rx.try_recv() {
        if matches!(evt, DaemonEvent::SpaceDiscovered { .. }) {
            count += 1;
        }
    }
    assert_eq!(
        count, 0,
        "dismissed space should not trigger SpaceDiscovered"
    );
}

#[tokio::test]
async fn poller_emits_space_removed_for_gone_folder() {
    let account_id = Uuid::new_v4();
    let folder_id = Uuid::new_v4();
    let server = make_server_with_spaces(r#"{"value":[]}"#).await;
    let tm = make_tm(&server, account_id).await;

    let config = Arc::new(Mutex::new(AppConfig {
        general: GeneralConfig::default(),
        account: vec![AccountConfig {
            id: account_id,
            url: server.uri(),
            user_id: "alice".into(),
            username: "alice".into(),
            display_name: "Alice".into(),
            folder: vec![FolderConfig {
                id: folder_id,
                local_path: "/tmp/ownCloud/OldSpace".into(),
                space_id: "gone-space".into(),
                display_name: "OldSpace".into(),
                selective_sync_excluded: vec![],
                vfs_mode: "off".into(),
                paused: false,
            }],
            dismissed_spaces: vec![],
        }],
    }));

    let (ipc, mut rx) = GuiIpcServer::new();
    let cancel = CancellationToken::new();
    let poller = SpacePoller::new(
        account_id,
        Arc::clone(&config),
        Arc::new(std::path::PathBuf::from("/tmp/test.toml")),
        Arc::clone(&ipc),
        tm,
        Duration::from_millis(50),
        cancel.clone(),
    );

    tokio::spawn(async move { poller.run().await });

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let removed = loop {
        if let Ok(evt) = rx.try_recv() {
            if matches!(evt, DaemonEvent::SpaceRemoved { .. }) {
                break evt;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("SpaceRemoved not received within 3s");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    cancel.cancel();

    match removed {
        DaemonEvent::SpaceRemoved {
            folder_id: fid,
            space_name,
            ..
        } => {
            assert_eq!(fid, folder_id);
            assert_eq!(space_name, "OldSpace");
        }
        _ => panic!("wrong event"),
    }

    let cfg = config.lock().await;
    let acc = cfg.account.iter().find(|a| a.id == account_id).unwrap();
    assert!(
        acc.folder.is_empty(),
        "removed folder should be gone from config"
    );
}

use camino::Utf8Path;
use std::io::Write;
use std::sync::Arc;
use sync_db::SyncJournalDb;
use sync_engine::engine::{EngineConfig, SyncEngine};
use sync_engine::types::ConflictStrategy;
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{header, method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ocis_client::auth::oidc::TokenSet;
use ocis_client::auth::OidcAuth;
use ocis_client::auth::TokenManager;

const DISCOVERY_DOC: &str = r#"{
    "issuer": "https://example.com",
    "authorization_endpoint": "https://example.com/auth",
    "token_endpoint": "https://example.com/token"
}"#;

async fn stub_token_manager(server: &MockServer) -> Arc<TokenManager> {
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DISCOVERY_DOC))
        .mount(server)
        .await;
    let oidc = OidcAuth::discover(
        &server.uri(),
        "test-client",
        None,
        "http://localhost:9999/callback",
        false,
    )
    .await
    .expect("OidcAuth::discover in test");
    let token = TokenSet {
        access_token: "test-token".into(),
        refresh_token: None,
        expires_at: i64::MAX,
    };
    Arc::new(TokenManager::new(oidc, token, "test-account"))
}

fn propfind_one_file(_server_uri: &str) -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/s1/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/s1/remote.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>12</D:getcontentlength>
        <D:getetag>"remote_etag"</D:getetag>
        <OC:fileid>file-remote-1</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#
}

#[tokio::test]
async fn engine_downloads_new_remote_file() {
    let server = MockServer::start().await;

    let token_manager = stub_token_manager(&server).await;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/s1.*"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(
            ResponseTemplate::new(207)
                .set_body_string(propfind_one_file(&server.uri()).to_string()),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/dav/spaces/s1/remote.txt"))
        .and(header("Authorization", "Bearer test-token")) // ← add this line
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"remote content")
                .insert_header("etag", r#""remote_etag""#),
        )
        .expect(1)
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let local_root = Utf8Path::from_path(dir.path()).unwrap().to_owned();
    let space_root = Url::parse(&format!("{}/dav/spaces/s1/", server.uri())).unwrap();

    let db_file = tempfile::NamedTempFile::new().unwrap();
    let db = SyncJournalDb::open(db_file.path()).await.unwrap();
    let cfg = EngineConfig {
        folder_id: Uuid::new_v4(),
        local_root: local_root.clone(),
        space_root,
        conflict_strategy: ConflictStrategy::KeepBoth,
        max_parallel_transfers: 3,
        db,
        token_manager,
    };

    let engine = SyncEngine::new(cfg);
    let report = engine.run_sync().await.unwrap();
    assert_eq!(report.remote_entries, 1);
    assert_eq!(report.local_entries, 0);
    assert_eq!(report.downloads, 1);
    assert_eq!(report.uploads, 0);
    assert!(report.errors.is_empty());
    assert!(
        report.http_events.len() >= 2,
        "expected PROPFIND + GET events"
    );
    assert!(report.http_events.iter().any(|e| e.method == "PROPFIND"));
    assert!(report
        .http_events
        .iter()
        .any(|e| e.method == "GET" && e.status == 200));

    let dest = local_root.join("remote.txt");
    assert!(dest.exists(), "remote.txt should have been downloaded");

    server.verify().await;
}

#[tokio::test]
async fn engine_uploads_new_local_file() {
    let server = MockServer::start().await;

    let token_manager = stub_token_manager(&server).await;

    let empty_propfind = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/spaces/s2/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/s2.*"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(207).set_body_string(empty_propfind))
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/dav/spaces/s2/local.txt"))
        .and(header("Authorization", "Bearer test-token")) // ← add this line
        .respond_with(ResponseTemplate::new(201).insert_header("etag", r#""up_etag""#))
        .expect(1)
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let local_root = Utf8Path::from_path(dir.path()).unwrap().to_owned();

    let mut f = std::fs::File::create(dir.path().join("local.txt")).unwrap();
    f.write_all(b"local data").unwrap();

    let space_root = Url::parse(&format!("{}/dav/spaces/s2/", server.uri())).unwrap();

    let db_file = tempfile::NamedTempFile::new().unwrap();
    let db = SyncJournalDb::open(db_file.path()).await.unwrap();
    let cfg = EngineConfig {
        folder_id: Uuid::new_v4(),
        local_root,
        space_root,
        conflict_strategy: ConflictStrategy::KeepBoth,
        max_parallel_transfers: 3,
        db,
        token_manager,
    };

    let engine = SyncEngine::new(cfg);
    let report = engine.run_sync().await.unwrap();
    assert_eq!(report.uploads, 1);
    assert_eq!(report.downloads, 0);
    assert!(report.errors.is_empty());
    assert!(report
        .http_events
        .iter()
        .any(|e| e.method == "PUT" && e.status == 201));

    server.verify().await;
}

#[tokio::test]
async fn engine_creates_remote_dir_on_upload() {
    let dir = TempDir::new().unwrap();
    let local_root = Utf8Path::from_path(dir.path()).unwrap();

    // Create a local subdirectory
    std::fs::create_dir(dir.path().join("photos")).unwrap();

    let server = MockServer::start().await;

    // PROPFIND returns empty space root (no remote entries)
    let empty_propfind = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/spaces/testspace/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .respond_with(ResponseTemplate::new(207).set_body_string(empty_propfind))
        .mount(&server)
        .await;

    // MKCOL for photos/ returns 201
    Mock::given(method("MKCOL"))
        .and(path("/dav/spaces/testspace/photos"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let space_root = Url::parse(&format!("{}/dav/spaces/testspace/", server.uri())).unwrap();
    let db_file = tempfile::NamedTempFile::new().unwrap();
    let db = SyncJournalDb::open(db_file.path()).await.unwrap();
    let token_manager = stub_token_manager(&server).await;

    let cfg = EngineConfig {
        folder_id: Uuid::new_v4(),
        local_root: local_root.to_owned(),
        space_root,
        conflict_strategy: ConflictStrategy::KeepBoth,
        max_parallel_transfers: 2,
        db,
        token_manager,
    };

    let engine = SyncEngine::new(cfg);
    let report = engine.run_sync().await.unwrap();

    assert_eq!(report.uploads, 1, "expected 1 upload (the photos dir)");
    assert!(
        report.errors.is_empty(),
        "expected no errors: {:?}",
        report.errors
    );

    server.verify().await;
}

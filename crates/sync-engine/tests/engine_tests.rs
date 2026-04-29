use camino::Utf8Path;
use std::io::Write;
use sync_engine::engine::{EngineConfig, SyncEngine};
use sync_engine::types::ConflictStrategy;
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/s1.*"))
        .respond_with(ResponseTemplate::new(207).set_body_string(propfind_one_file(&server.uri()).to_string()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/dav/spaces/s1/remote.txt"))
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

    let cfg = EngineConfig {
        folder_id: Uuid::new_v4(),
        local_root: local_root.clone(),
        space_root,
        conflict_strategy: ConflictStrategy::KeepBoth,
        max_parallel_transfers: 3,
    };

    let engine = SyncEngine::new(cfg);
    engine.run_sync().await.unwrap();

    let dest = local_root.join("remote.txt");
    assert!(dest.exists(), "remote.txt should have been downloaded");

    server.verify().await;
}

#[tokio::test]
async fn engine_uploads_new_local_file() {
    let server = MockServer::start().await;

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
        .respond_with(ResponseTemplate::new(207).set_body_string(empty_propfind))
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/dav/spaces/s2/local.txt"))
        .respond_with(ResponseTemplate::new(201).insert_header("etag", r#""up_etag""#))
        .expect(1)
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let local_root = Utf8Path::from_path(dir.path()).unwrap().to_owned();

    let mut f = std::fs::File::create(dir.path().join("local.txt")).unwrap();
    f.write_all(b"local data").unwrap();

    let space_root = Url::parse(&format!("{}/dav/spaces/s2/", server.uri())).unwrap();

    let cfg = EngineConfig {
        folder_id: Uuid::new_v4(),
        local_root,
        space_root,
        conflict_strategy: ConflictStrategy::KeepBoth,
        max_parallel_transfers: 3,
    };

    let engine = SyncEngine::new(cfg);
    engine.run_sync().await.unwrap();

    server.verify().await;
}

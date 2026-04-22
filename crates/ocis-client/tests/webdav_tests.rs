// crates/ocis-client/tests/webdav_tests.rs
use std::sync::Arc;

use tokio::sync::RwLock;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ocis_client::auth::oidc::TokenSet;
use ocis_client::webdav::{ResourceType, WebDavClient};

fn dummy_token() -> Arc<RwLock<TokenSet>> {
    Arc::new(RwLock::new(TokenSet {
        access_token: "test-access-token".into(),
        refresh_token: None,
        expires_at: i64::MAX,
    }))
}

const PROPFIND_RESPONSE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:oc="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/personal/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getetag>"dir-etag-001"</D:getetag>
        <oc:fileid>dir-file-id-001</oc:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/personal/hello.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getetag>"abc123"</D:getetag>
        <D:getcontentlength>42</D:getcontentlength>
        <D:getlastmodified>Mon, 01 Jan 2024 12:00:00 GMT</D:getlastmodified>
        <oc:fileid>file-id-001</oc:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

#[tokio::test]
async fn test_propfind_parses_multistatus() {
    let server = MockServer::start().await;

    Mock::given(method("PROPFIND"))
        .and(path("/dav/spaces/personal/"))
        .and(header("Depth", "1"))
        .respond_with(
            ResponseTemplate::new(207)
                .set_body_raw(PROPFIND_RESPONSE, "application/xml; charset=utf-8"),
        )
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = WebDavClient::new(base_url, dummy_token());

    let entries = client.propfind("dav/spaces/personal/").await.unwrap();
    assert_eq!(entries.len(), 2);

    let dir = &entries[0];
    assert_eq!(dir.href, "/dav/spaces/personal/");
    assert_eq!(dir.resource_type, ResourceType::Directory);
    assert_eq!(dir.file_id.as_deref(), Some("dir-file-id-001"));

    let file = &entries[1];
    assert_eq!(file.href, "/dav/spaces/personal/hello.txt");
    assert_eq!(file.resource_type, ResourceType::File);
    assert_eq!(file.etag.as_deref(), Some("abc123"));
    assert_eq!(file.content_length, Some(42));
    assert_eq!(file.file_id.as_deref(), Some("file-id-001"));
}

#[tokio::test]
async fn test_propfind_retries_on_401() {
    let server = MockServer::start().await;

    Mock::given(method("PROPFIND"))
        .and(path("/dav/spaces/personal/"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("PROPFIND"))
        .and(path("/dav/spaces/personal/"))
        .respond_with(
            ResponseTemplate::new(207)
                .set_body_raw(PROPFIND_RESPONSE, "application/xml; charset=utf-8"),
        )
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let token = dummy_token();
    let client = WebDavClient::new(base_url, token.clone());

    {
        let mut t = token.write().await;
        t.access_token = "refreshed-token".into();
    }

    let entries = client.propfind("dav/spaces/personal/").await.unwrap();
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn test_delete_sends_correct_method() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/dav/spaces/personal/old.txt"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = WebDavClient::new(base_url, dummy_token());
    client.delete("dav/spaces/personal/old.txt").await.unwrap();
}

#[tokio::test]
async fn test_mkcol_creates_directory() {
    let server = MockServer::start().await;

    Mock::given(method("MKCOL"))
        .and(path("/dav/spaces/personal/new-dir/"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = WebDavClient::new(base_url, dummy_token());
    client.mkcol("dav/spaces/personal/new-dir/").await.unwrap();
}

#[tokio::test]
async fn test_move_sets_destination_header() {
    let server = MockServer::start().await;

    Mock::given(method("MOVE"))
        .and(path("/dav/spaces/personal/old.txt"))
        .and(header("Overwrite", "T"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = WebDavClient::new(base_url, dummy_token());
    client
        .move_("dav/spaces/personal/old.txt", "dav/spaces/personal/new.txt", true)
        .await
        .unwrap();
}

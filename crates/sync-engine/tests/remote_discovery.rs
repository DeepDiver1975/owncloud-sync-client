use sync_engine::discovery::remote::discover_remote;
use url::Url;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn propfind_response_root() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/space1/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getlastmodified>Mon, 01 Jan 2024 00:00:00 GMT</D:getlastmodified>
        <D:getcontentlength>0</D:getcontentlength>
        <D:getetag>"rootetag"</D:getetag>
        <OC:fileid>root-id</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/space1/hello.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getlastmodified>Mon, 01 Jan 2024 12:00:00 GMT</D:getlastmodified>
        <D:getcontentlength>5</D:getcontentlength>
        <D:getetag>"abc123"</D:getetag>
        <OC:fileid>file-id-1</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#
}

#[tokio::test]
async fn discovers_files_from_propfind() {
    let server = MockServer::start().await;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/space1.*"))
        .respond_with(ResponseTemplate::new(207).set_body_string(propfind_response_root()))
        .mount(&server)
        .await;

    let base = Url::parse(&format!("{}/dav/spaces/space1/", server.uri())).unwrap();
    let entries = discover_remote(&base).await.unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path.file_name(), Some("hello.txt"));
    assert_eq!(entries[0].etag, "abc123");
    assert_eq!(entries[0].size, 5);
    assert_eq!(entries[0].file_id, "file-id-1");
}

#[tokio::test]
async fn empty_collection_returns_empty() {
    let server = MockServer::start().await;

    let empty = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/spaces/empty/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/empty.*"))
        .respond_with(ResponseTemplate::new(207).set_body_string(empty))
        .mount(&server)
        .await;

    let base = Url::parse(&format!("{}/dav/spaces/empty/", server.uri())).unwrap();
    let entries = discover_remote(&base).await.unwrap();
    assert!(entries.is_empty());
}

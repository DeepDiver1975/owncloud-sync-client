use sync_engine::discovery::remote::discover_remote;
use url::Url;
use wiremock::matchers::{header, method, path_regex};
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
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(207).set_body_string(propfind_response_root()))
        .mount(&server)
        .await;

    let base = Url::parse(&format!("{}/dav/spaces/space1/", server.uri())).unwrap();
    let entries = discover_remote(&base, "test-token", &mut vec![])
        .await
        .unwrap();

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
        .and(header("Authorization", "Bearer my-token"))
        .respond_with(ResponseTemplate::new(207).set_body_string(empty))
        .mount(&server)
        .await;

    let base = Url::parse(&format!("{}/dav/spaces/empty/", server.uri())).unwrap();
    let entries = discover_remote(&base, "my-token", &mut vec![])
        .await
        .unwrap();
    assert!(entries.is_empty());
}

#[tokio::test]
async fn discover_remote_records_http_events() {
    let server = MockServer::start().await;

    let propfind_response = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/s1/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/s1/hello.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>5</D:getcontentlength>
        <D:getetag>"abc"</D:getetag>
        <OC:fileid>fid1</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/s1.*"))
        .respond_with(ResponseTemplate::new(207).set_body_string(propfind_response))
        .mount(&server)
        .await;

    let space_root = url::Url::parse(&format!("{}/dav/spaces/s1/", server.uri())).unwrap();
    let mut http_events = vec![];
    let entries = sync_engine::discovery::remote::discover_remote(
        &space_root,
        "test-token",
        &mut http_events,
    )
    .await
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(http_events.len(), 1);
    assert_eq!(http_events[0].method, "PROPFIND");
    assert_eq!(http_events[0].status, 207);
    assert!(http_events[0].duration_ms < 5000);
}

#[tokio::test]
async fn discovers_collections_as_dir_entries() {
    let server = MockServer::start().await;

    // Root request returns root + photos/ collection + img.jpg file
    let root_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/space1/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getcontentlength>0</D:getcontentlength>
        <D:getetag>"rootetag"</D:getetag>
        <OC:fileid>root-id</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/space1/photos/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getcontentlength>0</D:getcontentlength>
        <D:getetag>"photosetag"</D:getetag>
        <OC:fileid>photos-id</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/space1/photos/img.jpg</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>1234</D:getcontentlength>
        <D:getetag>"imgetag"</D:getetag>
        <OC:fileid>img-id</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    // Recursive request for photos/ returns just itself and img.jpg
    let photos_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/space1/photos/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getcontentlength>0</D:getcontentlength>
        <D:getetag>"photosetag"</D:getetag>
        <OC:fileid>photos-id</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/space1/photos/img.jpg</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>1234</D:getcontentlength>
        <D:getetag>"imgetag"</D:getetag>
        <OC:fileid>img-id</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/space1/$"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(207).set_body_string(root_xml))
        .mount(&server)
        .await;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/space1/photos/"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(207).set_body_string(photos_xml))
        .mount(&server)
        .await;

    let base = Url::parse(&format!("{}/dav/spaces/space1/", server.uri())).unwrap();
    let entries = discover_remote(&base, "test-token", &mut vec![])
        .await
        .unwrap();

    let dir_entries: Vec<_> = entries.iter().filter(|e| e.is_dir).collect();
    assert_eq!(dir_entries.len(), 1, "expected one dir entry (photos/)");
    assert_eq!(
        dir_entries[0].path.as_str(),
        "photos",
        "dir path should be 'photos'"
    );

    let file_entries: Vec<_> = entries.iter().filter(|e| !e.is_dir).collect();
    assert_eq!(file_entries.len(), 1, "expected one file entry (img.jpg)");
    assert_eq!(file_entries[0].path.file_name(), Some("img.jpg"));
}

#[tokio::test]
async fn discovers_file_with_encoded_name() {
    let server = MockServer::start().await;

    // href encodes: "café 文件 <1>.txt"
    // café = caf\u{e9}, space = %20, 文件 = %E6%96%87%E4%BB%B6, space, <1> = %3C1%3E
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/space1/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/space1/caf%C3%A9%20%E6%96%87%E4%BB%B6%20%3C1%3E.txt</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>7</D:getcontentlength>
        <D:getetag>"etag1"</D:getetag>
        <OC:fileid>fid1</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/space1.*"))
        .and(header("Authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(207).set_body_string(xml))
        .mount(&server)
        .await;

    let base = Url::parse(&format!("{}/dav/spaces/space1/", server.uri())).unwrap();
    let entries = discover_remote(&base, "tok", &mut vec![]).await.unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path.as_str(), "café 文件 <1>.txt");
    assert!(
        !entries[0].path.as_str().contains('%'),
        "path must not contain percent-encoding"
    );
}

#[tokio::test]
async fn discovers_dir_with_encoded_name() {
    let server = MockServer::start().await;

    // href encodes: "my folder 📁/"
    // space = %20, 📁 = %F0%9F%93%81
    let root_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/space1/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/spaces/space1/my%20folder%20%F0%9F%93%81/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getcontentlength>0</D:getcontentlength>
        <D:getetag>"diretag"</D:getetag>
        <OC:fileid>dirid1</OC:fileid>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    // The recursive PROPFIND for the subdir returns just itself (no children)
    let sub_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:response>
    <D:href>/dav/spaces/space1/my%20folder%20%F0%9F%93%81/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/space1/$"))
        .and(header("Authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(207).set_body_string(root_xml))
        .mount(&server)
        .await;

    Mock::given(method("PROPFIND"))
        .and(path_regex(r"^/dav/spaces/space1/my"))
        .and(header("Authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(207).set_body_string(sub_xml))
        .mount(&server)
        .await;

    let base = Url::parse(&format!("{}/dav/spaces/space1/", server.uri())).unwrap();
    let entries = discover_remote(&base, "tok", &mut vec![]).await.unwrap();

    let dir_entries: Vec<_> = entries.iter().filter(|e| e.is_dir).collect();
    assert_eq!(dir_entries.len(), 1);
    assert_eq!(dir_entries[0].path.as_str(), "my folder 📁");
    assert!(
        !dir_entries[0].path.as_str().contains('%'),
        "dir path must not contain percent-encoding"
    );
}

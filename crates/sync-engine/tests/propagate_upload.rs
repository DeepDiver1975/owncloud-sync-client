use std::io::Write;
use sync_engine::propagate::upload::{propagate_upload, UploadRequest};
use sync_engine::report::HttpEvent;
use tempfile::NamedTempFile;
use tempfile::TempDir;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn small_file_uses_plain_put() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/dav/spaces/space1/hello.txt"))
        .respond_with(ResponseTemplate::new(201).insert_header("etag", r#""newetag""#))
        .expect(1)
        .mount(&server)
        .await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"hello").unwrap();
    tmp.flush().unwrap();

    let req = UploadRequest {
        local_path: camino::Utf8Path::from_path(tmp.path()).unwrap().to_owned(),
        remote_url: Url::parse(&format!("{}/dav/spaces/space1/hello.txt", server.uri())).unwrap(),
        space_root: Url::parse(&format!("{}/dav/spaces/space1/", server.uri())).unwrap(),
        size: 5,
        checksum: None,
        tus_threshold: 1024 * 1024 * 5,
        bearer_token: String::new(),
    };

    let mut http_events = vec![];
    let etag = propagate_upload(req, &mut http_events).await.unwrap();
    assert_eq!(etag.trim_matches('"'), "newetag");

    server.verify().await;
}

#[tokio::test]
async fn large_file_uses_tus_protocol() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/tus/upload"))
        .respond_with(ResponseTemplate::new(201).insert_header("location", "/tus/upload/abc123"))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("PATCH"))
        .and(path("/tus/upload/abc123"))
        .respond_with(
            ResponseTemplate::new(204)
                .insert_header("upload-offset", "6")
                .insert_header("etag", r#""tusetag""#),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"big!!!").unwrap();
    tmp.flush().unwrap();

    let req = UploadRequest {
        local_path: camino::Utf8Path::from_path(tmp.path()).unwrap().to_owned(),
        remote_url: Url::parse(&format!("{}/tus/upload", server.uri())).unwrap(),
        space_root: Url::parse(&format!("{}/dav/spaces/space1/", server.uri())).unwrap(),
        size: 6,
        checksum: None,
        tus_threshold: 4, // force TUS for any file > 4 bytes
        bearer_token: String::new(),
    };

    let mut http_events = vec![];
    let etag = propagate_upload(req, &mut http_events).await.unwrap();
    assert_eq!(etag.trim_matches('"'), "tusetag");

    server.verify().await;
}

#[tokio::test]
async fn upload_put_records_http_event() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/dav/spaces/s1/up.txt"))
        .respond_with(ResponseTemplate::new(201).insert_header("etag", r#""etag2""#))
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let local = dir.path().join("up.txt");
    std::fs::write(&local, b"world").unwrap();
    let local_path = camino::Utf8PathBuf::from_path_buf(local).unwrap();
    let remote_url = Url::parse(&format!("{}/dav/spaces/s1/up.txt", server.uri())).unwrap();

    let mut http_events: Vec<HttpEvent> = vec![];
    propagate_upload(
        UploadRequest {
            local_path,
            remote_url,
            space_root: Url::parse(&format!("{}/dav/spaces/space1/", server.uri())).unwrap(),
            size: 5,
            checksum: None,
            tus_threshold: 5 * 1024 * 1024,
            bearer_token: String::new(),
        },
        &mut http_events,
    )
    .await
    .unwrap();

    assert_eq!(http_events.len(), 1);
    assert_eq!(http_events[0].method, "PUT");
    assert_eq!(http_events[0].status, 201);
}

#[tokio::test]
async fn put_retries_after_409_with_mkcol() {
    let server = MockServer::start().await;

    // First PUT returns 409
    Mock::given(method("PUT"))
        .and(path("/dav/spaces/s1/photos/img.jpg"))
        .respond_with(ResponseTemplate::new(409))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // MKCOL for photos/ returns 201
    Mock::given(method("MKCOL"))
        .and(path("/dav/spaces/s1/photos/"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    // Retry PUT returns 201
    Mock::given(method("PUT"))
        .and(path("/dav/spaces/s1/photos/img.jpg"))
        .respond_with(ResponseTemplate::new(201).insert_header("etag", r#""img123""#))
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let local = dir.path().join("img.jpg");
    std::fs::write(&local, b"img").unwrap();

    let req = UploadRequest {
        local_path: camino::Utf8PathBuf::from_path_buf(local).unwrap(),
        remote_url: Url::parse(&format!("{}/dav/spaces/s1/photos/img.jpg", server.uri())).unwrap(),
        space_root: Url::parse(&format!("{}/dav/spaces/s1/", server.uri())).unwrap(),
        size: 3,
        checksum: None,
        tus_threshold: 5 * 1024 * 1024,
        bearer_token: String::new(),
    };

    let mut events = vec![];
    let etag = propagate_upload(req, &mut events).await.unwrap();
    assert_eq!(etag.trim_matches('"'), "img123");
    assert_eq!(events.len(), 2, "expected 2 PUT events (initial + retry)");

    server.verify().await;
}

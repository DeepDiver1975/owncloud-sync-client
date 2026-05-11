use camino::Utf8Path;
use sync_engine::propagate::download::{propagate_download, DownloadRequest};
use tempfile::TempDir;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn downloads_file_atomically() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/dav/spaces/space1/notes.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"file content here")
                .insert_header("etag", r#""dl_etag""#),
        )
        .expect(1)
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let dest = Utf8Path::from_path(dir.path()).unwrap().join("notes.txt");

    let req = DownloadRequest {
        remote_url: Url::parse(&format!("{}/dav/spaces/space1/notes.txt", server.uri())).unwrap(),
        local_dest: dest.clone(),
        expected_etag: None,
    };

    let mut http_events = vec![];
    let etag = propagate_download(req, &mut http_events).await.unwrap();

    let content = tokio::fs::read_to_string(&dest).await.unwrap();
    assert_eq!(content, "file content here");
    assert_eq!(etag.trim_matches('"'), "dl_etag");

    server.verify().await;
}

#[tokio::test]
async fn fails_on_etag_mismatch() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/dav/spaces/space1/stale.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"data")
                .insert_header("etag", r#""server_etag""#),
        )
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let dest = Utf8Path::from_path(dir.path()).unwrap().join("stale.txt");

    let req = DownloadRequest {
        remote_url: Url::parse(&format!("{}/dav/spaces/space1/stale.txt", server.uri())).unwrap(),
        local_dest: dest.clone(),
        expected_etag: Some("expected_different_etag".into()),
    };

    let mut http_events = vec![];
    let result = propagate_download(req, &mut http_events).await;
    assert!(result.is_err(), "should fail on etag mismatch");

    // Destination file must NOT exist (temp file was cleaned up).
    assert!(!dest.exists());
}

#[tokio::test]
async fn download_records_http_event() {
    use sync_engine::propagate::download::{propagate_download, DownloadRequest};
    use sync_engine::report::HttpEvent;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/dav/spaces/s1/file.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"hello")
                .insert_header("etag", r#""etag1""#),
        )
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let dest = camino::Utf8PathBuf::from_path_buf(dir.path().join("file.txt")).unwrap();
    let remote_url = url::Url::parse(&format!("{}/dav/spaces/s1/file.txt", server.uri())).unwrap();

    let mut http_events: Vec<HttpEvent> = vec![];
    propagate_download(
        DownloadRequest {
            remote_url,
            local_dest: dest,
            expected_etag: None,
        },
        &mut http_events,
    )
    .await
    .unwrap();

    assert_eq!(http_events.len(), 1);
    assert_eq!(http_events[0].method, "GET");
    assert_eq!(http_events[0].status, 200);
    assert_eq!(http_events[0].bytes, 5);
}

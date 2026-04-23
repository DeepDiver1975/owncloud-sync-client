use std::io::Write;
use sync_engine::propagate::upload::{propagate_upload, UploadRequest};
use tempfile::NamedTempFile;
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
        size: 5,
        checksum: None,
        tus_threshold: 1024 * 1024 * 5,
    };

    let etag = propagate_upload(req).await.unwrap();
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
        size: 6,
        checksum: None,
        tus_threshold: 4, // force TUS for any file > 4 bytes
    };

    let etag = propagate_upload(req).await.unwrap();
    assert_eq!(etag.trim_matches('"'), "tusetag");

    server.verify().await;
}

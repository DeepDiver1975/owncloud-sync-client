// crates/ocis-client/tests/tus_tests.rs
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ocis_client::auth::oidc::TokenSet;
use ocis_client::tus::TusClient;

fn dummy_token() -> Arc<RwLock<TokenSet>> {
    Arc::new(RwLock::new(TokenSet {
        access_token: "tus-test-token".into(),
        refresh_token: None,
        expires_at: i64::MAX,
    }))
}

#[tokio::test]
async fn test_tus_create_returns_upload_state() {
    let server = MockServer::start().await;
    let upload_url = format!("{}/tus/uploads/abc-upload-id", server.uri());

    Mock::given(method("POST"))
        .and(path("/tus/files/"))
        .and(header("Tus-Resumable", "1.0.0"))
        .and(header("Upload-Length", "1024"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("Location", upload_url.as_str())
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    let client = TusClient::new(dummy_token());
    let endpoint: url::Url = format!("{}/tus/files/", server.uri()).parse().unwrap();

    let mut metadata = HashMap::new();
    metadata.insert("content-type".to_string(), "text/plain".to_string());

    let upload = client
        .create(&endpoint, "/documents/report.txt", 1024, metadata)
        .await
        .unwrap();

    assert_eq!(upload.offset, 0);
    assert_eq!(upload.total_size, 1024);
    assert!(upload.upload_url.as_str().contains("abc-upload-id"));
}

#[tokio::test]
async fn test_tus_upload_chunk_updates_offset() {
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/tus/uploads/abc-upload-id"))
        .and(header("Tus-Resumable", "1.0.0"))
        .and(header("Upload-Offset", "0"))
        .respond_with(
            ResponseTemplate::new(204)
                .insert_header("Upload-Offset", "512")
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    let client = TusClient::new(dummy_token());
    let upload_url: url::Url = format!("{}/tus/uploads/abc-upload-id", server.uri())
        .parse()
        .unwrap();

    let mut upload = ocis_client::tus::TusUpload {
        upload_url,
        offset: 0,
        total_size: 1024,
    };

    let data = vec![0u8; 512];
    client.upload_chunk(&mut upload, &data).await.unwrap();

    assert_eq!(upload.offset, 512);
}

#[tokio::test]
async fn test_tus_resume_reads_offset_from_head() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/tus/uploads/abc-upload-id"))
        .and(header("Tus-Resumable", "1.0.0"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Upload-Offset", "256")
                .insert_header("Upload-Length", "1024")
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    let client = TusClient::new(dummy_token());
    let upload_url: url::Url = format!("{}/tus/uploads/abc-upload-id", server.uri())
        .parse()
        .unwrap();

    let upload = client.resume(&upload_url).await.unwrap();

    assert_eq!(upload.offset, 256);
    assert_eq!(upload.total_size, 1024);
    assert_eq!(upload.upload_url, upload_url);
}

#[tokio::test]
async fn test_tus_full_sequence() {
    let server = MockServer::start().await;
    let upload_url_str = format!("{}/tus/uploads/full-seq-id", server.uri());

    Mock::given(method("POST"))
        .and(path("/tus/files/"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("Location", upload_url_str.as_str())
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    Mock::given(method("PATCH"))
        .and(path("/tus/uploads/full-seq-id"))
        .and(header("Upload-Offset", "0"))
        .respond_with(
            ResponseTemplate::new(204)
                .insert_header("Upload-Offset", "512")
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("PATCH"))
        .and(path("/tus/uploads/full-seq-id"))
        .and(header("Upload-Offset", "512"))
        .respond_with(
            ResponseTemplate::new(204)
                .insert_header("Upload-Offset", "1024")
                .insert_header("Tus-Resumable", "1.0.0"),
        )
        .mount(&server)
        .await;

    let client = TusClient::new(dummy_token());
    let endpoint: url::Url = format!("{}/tus/files/", server.uri()).parse().unwrap();

    let mut upload = client
        .create(&endpoint, "/video.mp4", 1024, HashMap::new())
        .await
        .unwrap();

    assert_eq!(upload.offset, 0);

    let chunk1 = vec![0xABu8; 512];
    client.upload_chunk(&mut upload, &chunk1).await.unwrap();
    assert_eq!(upload.offset, 512);

    let chunk2 = vec![0xCDu8; 512];
    client.upload_chunk(&mut upload, &chunk2).await.unwrap();
    assert_eq!(upload.offset, 1024);
}

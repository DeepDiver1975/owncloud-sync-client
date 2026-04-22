// crates/ocis-client/tests/graph_tests.rs
use std::sync::Arc;

use tokio::sync::RwLock;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ocis_client::auth::oidc::TokenSet;
use ocis_client::graph::{webdav_url_for_space, GraphClient};

fn dummy_token() -> Arc<RwLock<TokenSet>> {
    Arc::new(RwLock::new(TokenSet {
        access_token: "test-token".into(),
        refresh_token: None,
        expires_at: i64::MAX,
    }))
}

const LIST_SPACES_RESPONSE: &str = r#"{
  "value": [
    {
      "id": "storage-personal-abc123",
      "name": "Personal",
      "driveType": "personal",
      "webUrl": "https://ocis.example.com/personal",
      "quota": {
        "total": 10737418240,
        "used": 104857600,
        "remaining": 10632560640
      }
    },
    {
      "id": "storage-project-xyz",
      "name": "Project Alpha",
      "driveType": "project",
      "webUrl": "https://ocis.example.com/drives/project-alpha",
      "quota": null
    }
  ]
}"#;

const GET_SPACE_RESPONSE: &str = r#"{
  "id": "storage-personal-abc123",
  "name": "Personal",
  "driveType": "personal",
  "webUrl": "https://ocis.example.com/personal",
  "quota": {
    "total": 10737418240,
    "used": 104857600,
    "remaining": 10632560640
  }
}"#;

#[tokio::test]
async fn test_list_spaces_parses_json() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/graph/v1.0/me/drives"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(LIST_SPACES_RESPONSE, "application/json"),
        )
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = GraphClient::new(base_url, dummy_token());

    let spaces = client.list_spaces().await.unwrap();
    assert_eq!(spaces.len(), 2);

    let personal = &spaces[0];
    assert_eq!(personal.id, "storage-personal-abc123");
    assert_eq!(personal.name, "Personal");
    assert_eq!(personal.drive_type, "personal");
    let quota = personal.quota.as_ref().unwrap();
    assert_eq!(quota.total, 10737418240);
    assert_eq!(quota.used, 104857600);

    let project = &spaces[1];
    assert_eq!(project.id, "storage-project-xyz");
    assert!(project.quota.is_none());
}

#[tokio::test]
async fn test_get_space_by_id() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/graph/v1.0/drives/storage-personal-abc123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(GET_SPACE_RESPONSE, "application/json"),
        )
        .mount(&server)
        .await;

    let base_url = format!("{}/", server.uri()).parse().unwrap();
    let client = GraphClient::new(base_url, dummy_token());

    let space = client.get_space("storage-personal-abc123").await.unwrap();
    assert_eq!(space.name, "Personal");
}

#[tokio::test]
async fn test_webdav_url_for_space() {
    let server_url: url::Url = "https://ocis.example.com/".parse().unwrap();
    let space_id = "storage$personal!abc-123";
    let url = webdav_url_for_space(&server_url, space_id).unwrap();
    assert_eq!(
        url.as_str(),
        "https://ocis.example.com/dav/spaces/storage$personal!abc-123/"
    );
}

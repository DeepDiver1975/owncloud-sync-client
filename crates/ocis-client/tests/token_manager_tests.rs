// crates/ocis-client/tests/token_manager_tests.rs
use std::time::{SystemTime, UNIX_EPOCH};

use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ocis_client::auth::oidc::{OidcAuth, TokenSet};
use ocis_client::auth::token_manager::TokenManager;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn fresh_token() -> TokenSet {
    TokenSet {
        access_token: "fresh-access".into(),
        refresh_token: Some("refresh-tok".into()),
        expires_at: now_secs() + 3600,
    }
}

fn expired_token() -> TokenSet {
    TokenSet {
        access_token: "stale-access".into(),
        refresh_token: Some("refresh-tok".into()),
        expires_at: now_secs() - 10, // already expired
    }
}

fn expired_no_refresh() -> TokenSet {
    TokenSet {
        access_token: "stale-access".into(),
        refresh_token: None,
        expires_at: now_secs() - 10,
    }
}

const TOKEN_RESPONSE: &str = r#"{
    "access_token": "new-access-token",
    "refresh_token": "new-refresh-token",
    "expires_in": 3600
}"#;

async fn make_oidc(server: &MockServer) -> OidcAuth {
    let base = server.uri();
    let discovery_doc = format!(
        r#"{{
            "issuer": "{base}",
            "authorization_endpoint": "{base}/auth",
            "token_endpoint": "{base}/token"
        }}"#
    );

    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_string(discovery_doc))
        .mount(server)
        .await;

    OidcAuth::discover(
        &base,
        "test-client",
        None,
        "http://localhost:9999/callback",
        false,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn unexpired_token_returned_without_refresh() {
    let server = MockServer::start().await;
    let oidc = make_oidc(&server).await;

    let tm = TokenManager::new(oidc, fresh_token(), "acct-1".to_string());
    let token = tm.get_valid_token().await.unwrap();

    assert_eq!(token, "fresh-access");
    // No requests to /token — the mock server would record them if any.
    let received = server.received_requests().await.unwrap();
    // Only the discovery request, no token requests.
    assert!(received
        .iter()
        .all(|r| r.url.path() == "/.well-known/openid-configuration"));
}

#[tokio::test]
async fn expired_token_is_refreshed() {
    let server = MockServer::start().await;
    let oidc = make_oidc(&server).await;

    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TOKEN_RESPONSE))
        .mount(&server)
        .await;

    let tm = TokenManager::new(oidc, expired_token(), "acct-2".to_string());
    let token = tm.get_valid_token().await.unwrap();

    assert_eq!(token, "new-access-token");

    // Arc should also be updated.
    let stored = tm.token_arc().read().await.access_token.clone();
    assert_eq!(stored, "new-access-token");
}

#[tokio::test]
async fn expired_token_without_refresh_token_returns_err() {
    let server = MockServer::start().await;
    let oidc = make_oidc(&server).await;

    let tm = TokenManager::new(oidc, expired_no_refresh(), "acct-3".to_string());
    let result = tm.get_valid_token().await;

    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("no refresh token"), "got: {msg}");
}

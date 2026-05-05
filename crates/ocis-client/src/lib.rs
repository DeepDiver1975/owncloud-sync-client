// crates/ocis-client/src/lib.rs
pub mod auth;
pub mod error;
pub mod graph;
pub mod tus;
pub mod webdav;

pub use error::{OcisError, Result};
pub use graph::{webdav_url_for_space, GraphClient, Space, SpaceQuota, UserInfo};
pub use tus::{TusClient, TusUpload};

/// Build a `reqwest::Client`, accepting invalid TLS certs when `OCIS_INSECURE=1` is set.
///
/// When `OCIS_BASIC_AUTH` is set to `user:password`, every request will include
/// a Basic Authorization header. This is used in acceptance tests where the sync
/// engine does not yet carry OIDC credentials.
pub fn build_http_client() -> reqwest::Client {
    let insecure = std::env::var("OCIS_INSECURE")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    let mut builder = reqwest::Client::builder().danger_accept_invalid_certs(insecure);

    if let Ok(basic) = std::env::var("OCIS_BASIC_AUTH") {
        if let Some((user, pass)) = basic.split_once(':') {
            use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
            let encoded = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                format!("{user}:{pass}"),
            );
            let mut headers = HeaderMap::new();
            if let Ok(val) = HeaderValue::from_str(&format!("Basic {encoded}")) {
                headers.insert(AUTHORIZATION, val);
            }
            builder = builder.default_headers(headers);
        }
    }

    builder.build().expect("build reqwest client")
}

// crates/ocis-client/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OcisError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("WebDAV error: {0}")]
    WebDav(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Keychain error: {0}")]
    Keychain(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, OcisError>;

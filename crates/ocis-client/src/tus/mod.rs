// crates/ocis-client/src/tus/mod.rs
//! TUS 1.0 resumable upload protocol implementation.

use std::collections::HashMap;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use reqwest::{Client, StatusCode};
use tokio::sync::RwLock;
use url::Url;

use crate::auth::oidc::TokenSet;
use crate::error::{OcisError, Result};

/// State for a single TUS upload session.
#[derive(Debug, Clone)]
pub struct TusUpload {
    pub upload_url: Url,
    pub offset: u64,
    pub total_size: u64,
}

/// TUS resumable upload client for oCIS.
#[derive(Debug, Clone)]
pub struct TusClient {
    client: Client,
    token: Arc<RwLock<TokenSet>>,
}

impl TusClient {
    pub fn new(token: Arc<RwLock<TokenSet>>) -> Self {
        let client = Client::builder()
            .use_rustls_tls()
            .build()
            .expect("build reqwest client");
        Self { client, token }
    }

    /// Initiate a new TUS upload.
    ///
    /// POST to `endpoint` with required TUS headers. The server responds with
    /// a `Location` header pointing at the upload URL.
    pub async fn create(
        &self,
        endpoint: &Url,
        path: &str,
        size: u64,
        metadata: HashMap<String, String>,
    ) -> Result<TusUpload> {
        let token = self.token.read().await.access_token.clone();

        let meta_header: String = {
            let mut pairs: Vec<String> = metadata
                .iter()
                .map(|(k, v)| format!("{} {}", k, BASE64_STANDARD.encode(v)))
                .collect();
            pairs.push(format!("filename {}", BASE64_STANDARD.encode(path)));
            pairs.join(", ")
        };

        let resp = self
            .client
            .post(endpoint.clone())
            .bearer_auth(&token)
            .header("Tus-Resumable", "1.0.0")
            .header("Upload-Length", size.to_string())
            .header("Upload-Metadata", meta_header)
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .send()
            .await
            .map_err(OcisError::Http)?;

        if resp.status() != StatusCode::CREATED {
            return Err(OcisError::WebDav(format!(
                "TUS create failed: {}",
                resp.status()
            )));
        }

        let location = resp
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| OcisError::WebDav("TUS create: missing Location header".into()))?;

        let upload_url: Url = location
            .parse()
            .map_err(|e: url::ParseError| OcisError::WebDav(e.to_string()))?;

        Ok(TusUpload {
            upload_url,
            offset: 0,
            total_size: size,
        })
    }

    /// Upload a chunk of data.
    ///
    /// PATCH to `upload.upload_url` with `data` bytes starting at `upload.offset`.
    /// On success `upload.offset` is advanced by the server-reported offset.
    pub async fn upload_chunk(&self, upload: &mut TusUpload, data: &[u8]) -> Result<()> {
        let token = self.token.read().await.access_token.clone();

        let resp = self
            .client
            .patch(upload.upload_url.clone())
            .bearer_auth(&token)
            .header("Tus-Resumable", "1.0.0")
            .header("Content-Type", "application/offset+octet-stream")
            .header("Upload-Offset", upload.offset.to_string())
            .header(reqwest::header::CONTENT_LENGTH, data.len().to_string())
            .body(data.to_vec())
            .send()
            .await
            .map_err(OcisError::Http)?;

        if resp.status() != StatusCode::NO_CONTENT {
            return Err(OcisError::WebDav(format!(
                "TUS upload_chunk failed: {}",
                resp.status()
            )));
        }

        let server_offset = resp
            .headers()
            .get("Upload-Offset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(upload.offset + data.len() as u64);

        upload.offset = server_offset;
        Ok(())
    }

    /// Resume an interrupted upload by querying the current server offset.
    ///
    /// HEAD to `upload_url`, reads the `Upload-Offset` header.
    pub async fn resume(&self, upload_url: &Url) -> Result<TusUpload> {
        let token = self.token.read().await.access_token.clone();

        let resp = self
            .client
            .head(upload_url.clone())
            .bearer_auth(&token)
            .header("Tus-Resumable", "1.0.0")
            .send()
            .await
            .map_err(OcisError::Http)?
            .error_for_status()
            .map_err(OcisError::Http)?;

        let offset = resp
            .headers()
            .get("Upload-Offset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| OcisError::WebDav("TUS resume: missing Upload-Offset header".into()))?;

        let total_size = resp
            .headers()
            .get("Upload-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        Ok(TusUpload {
            upload_url: upload_url.clone(),
            offset,
            total_size,
        })
    }
}

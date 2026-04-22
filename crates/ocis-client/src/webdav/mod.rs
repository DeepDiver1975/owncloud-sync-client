// crates/ocis-client/src/webdav/mod.rs
pub mod propfind;

pub use propfind::{parse_propfind_response, DavEntry, ResourceType};

use std::sync::Arc;

use reqwest::{header, Client, Method, StatusCode};
use tokio::sync::RwLock;
use url::Url;

use crate::auth::oidc::TokenSet;
use crate::error::{OcisError, Result};

/// HTTP client for WebDAV operations against an oCIS server.
#[derive(Debug, Clone)]
pub struct WebDavClient {
    pub base_url: Url,
    client: Client,
    token: Arc<RwLock<TokenSet>>,
}

impl WebDavClient {
    /// Create a new client. `token` is shared so callers can update it after a refresh.
    pub fn new(base_url: Url, token: Arc<RwLock<TokenSet>>) -> Self {
        let client = Client::builder()
            .use_rustls_tls()
            .build()
            .expect("build reqwest client");
        Self { base_url, client, token }
    }

    /// Issue a request, retrying once on 401 with the latest token value.
    async fn request_with_retry(
        &self,
        method: Method,
        url: Url,
        setup: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let access_token = self.token.read().await.access_token.clone();
        let req = setup(
            self.client
                .request(method.clone(), url.clone())
                .bearer_auth(&access_token),
        );

        let resp = req.send().await.map_err(OcisError::Http)?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            // The caller is expected to refresh the shared token on 401; re-reading picks up the new value.
            let new_token = self.token.read().await.access_token.clone();
            let retry = setup(
                self.client
                    .request(method, url)
                    .bearer_auth(&new_token),
            );
            return retry.send().await.map_err(OcisError::Http);
        }

        Ok(resp)
    }

    /// PROPFIND `depth=1` — list a collection.
    pub async fn propfind(&self, path: &str) -> Result<Vec<DavEntry>> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;

        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:oc="http://owncloud.org/ns">
  <D:prop>
    <D:getetag/>
    <D:getlastmodified/>
    <D:getcontentlength/>
    <D:resourcetype/>
    <oc:fileid/>
  </D:prop>
</D:propfind>"#;

        let resp = self
            .request_with_retry(Method::from_bytes(b"PROPFIND").unwrap(), url, |req| {
                req.header("Depth", "1")
                    .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
                    .body(body)
            })
            .await?;

        if resp.status().as_u16() != 207 {
            return Err(OcisError::WebDav(format!(
                "PROPFIND failed: {}",
                resp.status()
            )));
        }

        let text = resp.text().await.map_err(OcisError::Http)?;
        parse_propfind_response(&text)
    }

    /// GET — download a file, returns raw bytes.
    pub async fn get(&self, path: &str) -> Result<bytes::Bytes> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        let resp = self
            .request_with_retry(Method::GET, url, |req| req)
            .await?
            .error_for_status()
            .map_err(OcisError::Http)?;
        resp.bytes().await.map_err(OcisError::Http)
    }

    /// PUT — upload a file body.
    pub async fn put(
        &self,
        path: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<()> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        let len = body.len() as u64;

        self.request_with_retry(Method::PUT, url, |req| {
            req.header(header::CONTENT_TYPE, content_type)
                .header(header::CONTENT_LENGTH, len)
                .body(body.clone())
        })
        .await?
        .error_for_status()
        .map_err(OcisError::Http)?;

        Ok(())
    }

    /// DELETE — remove a resource.
    pub async fn delete(&self, path: &str) -> Result<()> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        self.request_with_retry(Method::DELETE, url, |req| req)
            .await?
            .error_for_status()
            .map_err(OcisError::Http)?;
        Ok(())
    }

    /// MKCOL — create a collection (directory).
    pub async fn mkcol(&self, path: &str) -> Result<()> {
        let url = self.base_url.join(path).map_err(OcisError::Url)?;
        self.request_with_retry(Method::from_bytes(b"MKCOL").unwrap(), url, |req| req)
            .await?
            .error_for_status()
            .map_err(OcisError::Http)?;
        Ok(())
    }

    /// MOVE — rename or move a resource.
    pub async fn move_(
        &self,
        source: &str,
        destination: &str,
        overwrite: bool,
    ) -> Result<()> {
        let url = self.base_url.join(source).map_err(OcisError::Url)?;
        let dest_url = self.base_url.join(destination).map_err(OcisError::Url)?;
        let overwrite_value = if overwrite { "T" } else { "F" };

        self.request_with_retry(Method::from_bytes(b"MOVE").unwrap(), url, |req| {
            req.header("Destination", dest_url.as_str())
                .header("Overwrite", overwrite_value)
        })
        .await?
        .error_for_status()
        .map_err(OcisError::Http)?;

        Ok(())
    }
}

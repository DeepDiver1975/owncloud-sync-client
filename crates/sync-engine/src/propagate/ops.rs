use camino::Utf8Path;
use url::Url;

use crate::error::{Result, SyncError};

/// Remove a file from the local filesystem.
pub async fn delete_local(path: &Utf8Path) -> Result<()> {
    tokio::fs::remove_file(path).await?;
    Ok(())
}

/// Send a WebDAV DELETE for `url`.
pub async fn delete_remote(url: &Url) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(url.as_str())
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let status = resp.status().as_u16();
    if status != 204 && status != 200 {
        return Err(SyncError::Http {
            status,
            message: format!("DELETE failed: {}", resp.status()),
        });
    }
    Ok(())
}

/// Send a WebDAV MKCOL for `url`.
pub async fn mkdir_remote(url: &Url) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), url.as_str())
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let status = resp.status().as_u16();
    if status != 201 && status != 200 && status != 405 {
        // 405 = already exists, which is fine.
        return Err(SyncError::Http {
            status,
            message: format!("MKCOL failed: {}", resp.status()),
        });
    }
    Ok(())
}

/// Send a WebDAV MOVE from `from_url` to `to_url`.
pub async fn rename_remote(from_url: &Url, to_url: &Url) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .request(reqwest::Method::from_bytes(b"MOVE").unwrap(), from_url.as_str())
        .header("Destination", to_url.as_str())
        .header("Overwrite", "T")
        .send()
        .await
        .map_err(|e| SyncError::Http { status: 0, message: e.to_string() })?;

    let status = resp.status().as_u16();
    if status != 201 && status != 204 {
        return Err(SyncError::Http {
            status,
            message: format!("MOVE failed: {}", resp.status()),
        });
    }
    Ok(())
}

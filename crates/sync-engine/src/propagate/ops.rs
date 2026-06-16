// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

use camino::Utf8Path;
use url::Url;

use crate::error::{Result, SyncError};

/// Remove a file from the local filesystem.
///
/// A path that is already gone is treated as success: a delete instruction may
/// race with a recursive parent-directory delete, and the desired end state
/// (path absent) already holds.
pub async fn delete_local(path: &Utf8Path) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Recursively remove a directory from the local filesystem.
///
/// As with [`delete_local`], an already-absent directory is treated as success.
pub async fn delete_local_dir(path: &Utf8Path) -> Result<()> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Send a WebDAV DELETE for `url`. For collections this removes the resource
/// recursively, server-side.
///
/// A `404 Not Found` is treated as success: the resource is already gone, which
/// is the desired end state (e.g. a child whose parent collection was deleted
/// first).
pub async fn delete_remote(url: &Url, bearer_token: &str) -> Result<()> {
    let client = ocis_client::build_http_client();
    let resp = client
        .delete(url.as_str())
        .bearer_auth(bearer_token)
        .send()
        .await
        .map_err(|e| SyncError::Http {
            status: 0,
            message: e.to_string(),
        })?;

    let status = resp.status().as_u16();
    if status != 204 && status != 200 && status != 404 {
        return Err(SyncError::Http {
            status,
            message: format!("DELETE failed: {}", resp.status()),
        });
    }
    Ok(())
}

/// Send a WebDAV MKCOL for `url`.
pub async fn mkdir_remote(url: Url, bearer_token: &str) -> Result<()> {
    let client = ocis_client::build_http_client();
    let resp = client
        .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), url.as_str())
        .bearer_auth(bearer_token)
        .send()
        .await
        .map_err(|e| SyncError::Http {
            status: 0,
            message: e.to_string(),
        })?;

    let status = resp.status().as_u16();
    if matches!(status, 201 | 200 | 405) {
        Ok(())
    } else {
        Err(SyncError::Http {
            status,
            message: "MKCOL failed".into(),
        })
    }
}

/// Send a WebDAV MOVE from `from_url` to `to_url`.
pub async fn rename_remote(from_url: &Url, to_url: &Url) -> Result<()> {
    let client = ocis_client::build_http_client();
    let resp = client
        .request(
            reqwest::Method::from_bytes(b"MOVE").unwrap(),
            from_url.as_str(),
        )
        .header("Destination", to_url.as_str())
        .header("Overwrite", "T")
        .send()
        .await
        .map_err(|e| SyncError::Http {
            status: 0,
            message: e.to_string(),
        })?;

    let status = resp.status().as_u16();
    if status != 201 && status != 204 {
        return Err(SyncError::Http {
            status,
            message: format!("MOVE failed: {}", resp.status()),
        });
    }
    Ok(())
}

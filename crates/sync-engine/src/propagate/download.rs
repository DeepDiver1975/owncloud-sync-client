use camino::Utf8PathBuf;
use tokio::io::AsyncWriteExt as _;
use url::Url;

use crate::error::{Result, SyncError};

pub struct DownloadRequest {
    pub remote_url: Url,
    pub local_dest: Utf8PathBuf,
    pub expected_etag: Option<String>,
}

/// Download `req.remote_url` to `req.local_dest`.
///
/// Streams into a `.tmp` sibling file and atomically renames on success.
/// If `expected_etag` is set and does not match the server's ETag, the temp
/// file is removed and an error is returned.
pub async fn propagate_download(req: DownloadRequest) -> Result<String> {
    let client = reqwest::Client::new();

    let resp = client
        .get(req.remote_url.as_str())
        .send()
        .await
        .map_err(|e| SyncError::Http {
            status: 0,
            message: e.to_string(),
        })?;

    let status = resp.status().as_u16();
    if status != 200 {
        return Err(SyncError::Http {
            status,
            message: format!("GET failed: {}", resp.status()),
        });
    }

    let server_etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if let Some(ref expected) = req.expected_etag {
        let stripped_server = server_etag.trim_matches('"');
        let stripped_expected = expected.trim_matches('"');
        if stripped_server != stripped_expected {
            return Err(SyncError::Parse(format!(
                "ETag mismatch: expected {expected}, got {server_etag}"
            )));
        }
    }

    if let Some(parent) = req.local_dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let tmp_path = req.local_dest.with_extension("tmp");
    {
        let bytes = resp.bytes().await.map_err(|e| SyncError::Http {
            status: 0,
            message: e.to_string(),
        })?;

        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
    }

    tokio::fs::rename(&tmp_path, &req.local_dest)
        .await
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            e
        })?;

    Ok(server_etag)
}

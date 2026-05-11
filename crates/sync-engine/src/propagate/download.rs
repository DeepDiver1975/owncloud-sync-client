use camino::Utf8PathBuf;
use tokio::io::AsyncWriteExt as _;
use url::Url;

use crate::error::{Result, SyncError};
use crate::report::HttpEvent;

pub struct DownloadRequest {
    pub remote_url: Url,
    pub local_dest: Utf8PathBuf,
    pub expected_etag: Option<String>,
}

pub async fn propagate_download(
    req: DownloadRequest,
    http_events: &mut Vec<HttpEvent>,
) -> Result<String> {
    let client = ocis_client::build_http_client();

    let t0 = tokio::time::Instant::now();
    let resp = client
        .get(req.remote_url.as_str())
        .send()
        .await
        .map_err(|e| SyncError::Http {
            status: 0,
            message: e.to_string(),
        })?;

    let status = resp.status().as_u16();
    let sanitised_url = {
        let mut u = req.remote_url.clone();
        u.set_query(None);
        u.set_fragment(None);
        u.to_string()
    };

    if status != 200 {
        http_events.push(HttpEvent {
            method: "GET".to_string(),
            url: sanitised_url,
            status,
            duration_ms: t0.elapsed().as_millis() as u64,
            bytes: 0,
        });
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
    let bytes = resp.bytes().await.map_err(|e| SyncError::Http {
        status: 0,
        message: e.to_string(),
    })?;
    let byte_count = bytes.len() as u64;

    {
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
    }

    tokio::fs::rename(&tmp_path, &req.local_dest)
        .await
        .inspect_err(|_e| {
            let _ = std::fs::remove_file(&tmp_path);
        })?;

    http_events.push(HttpEvent {
        method: "GET".to_string(),
        url: sanitised_url,
        status,
        duration_ms: t0.elapsed().as_millis() as u64,
        bytes: byte_count,
    });

    Ok(server_etag)
}

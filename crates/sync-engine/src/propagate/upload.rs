use camino::Utf8PathBuf;
use url::Url;

use crate::error::{Result, SyncError};

pub struct UploadRequest {
    pub local_path: Utf8PathBuf,
    pub remote_url: Url,
    pub size: u64,
    pub checksum: Option<String>,
    pub tus_threshold: u64,
}

pub async fn propagate_upload(req: UploadRequest) -> Result<String> {
    if req.size >= req.tus_threshold {
        upload_tus(req).await
    } else {
        upload_put(req).await
    }
}

async fn upload_put(req: UploadRequest) -> Result<String> {
    let bytes = tokio::fs::read(&req.local_path).await?;
    let client = reqwest::Client::new();

    let mut builder = client
        .put(req.remote_url.as_str())
        .header("Content-Length", req.size.to_string())
        .body(bytes);

    if let Some(ref cs) = req.checksum {
        builder = builder.header("OC-Checksum", format!("SHA256:{cs}"));
    }

    let resp = builder.send().await.map_err(|e| SyncError::Http {
        status: 0,
        message: e.to_string(),
    })?;

    let status = resp.status().as_u16();
    if status != 200 && status != 201 && status != 204 {
        return Err(SyncError::Http {
            status,
            message: format!("PUT failed: {}", resp.status()),
        });
    }

    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    Ok(etag)
}

async fn upload_tus(req: UploadRequest) -> Result<String> {
    let client = reqwest::Client::new();

    let create_resp = client
        .post(req.remote_url.as_str())
        .header("Tus-Resumable", "1.0.0")
        .header("Upload-Length", req.size.to_string())
        .header("Content-Length", "0")
        .send()
        .await
        .map_err(|e| SyncError::Http {
            status: 0,
            message: e.to_string(),
        })?;

    let status = create_resp.status().as_u16();
    if status != 201 {
        return Err(SyncError::Http {
            status,
            message: "TUS creation failed".into(),
        });
    }

    let location = create_resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| SyncError::Parse("TUS: missing Location header".into()))?
        .to_string();

    let patch_url = if location.starts_with("http://") || location.starts_with("https://") {
        location.clone()
    } else {
        let host = req.remote_url.host_str().unwrap_or("");
        match req.remote_url.port() {
            Some(port) => format!(
                "{}://{}:{}{}",
                req.remote_url.scheme(),
                host,
                port,
                location
            ),
            None => format!("{}://{}{}", req.remote_url.scheme(), host, location),
        }
    };

    let bytes = tokio::fs::read(&req.local_path).await?;

    let patch_resp = client
        .patch(&patch_url)
        .header("Tus-Resumable", "1.0.0")
        .header("Upload-Offset", "0")
        .header("Content-Type", "application/offset+octet-stream")
        .header("Content-Length", req.size.to_string())
        .body(bytes)
        .send()
        .await
        .map_err(|e| SyncError::Http {
            status: 0,
            message: e.to_string(),
        })?;

    let patch_status = patch_resp.status().as_u16();
    if patch_status != 204 && patch_status != 200 {
        return Err(SyncError::Http {
            status: patch_status,
            message: "TUS PATCH failed".into(),
        });
    }

    let etag = patch_resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    Ok(etag)
}

use camino::Utf8PathBuf;
use url::Url;

use crate::error::{Result, SyncError};
use crate::report::HttpEvent;

pub struct UploadRequest {
    pub local_path: Utf8PathBuf,
    pub remote_url: Url,
    pub size: u64,
    pub checksum: Option<String>,
    pub tus_threshold: u64,
}

pub async fn propagate_upload(
    req: UploadRequest,
    http_events: &mut Vec<HttpEvent>,
) -> Result<String> {
    if req.size >= req.tus_threshold {
        upload_tus(req, http_events).await
    } else {
        upload_put(req, http_events).await
    }
}

async fn upload_put(req: UploadRequest, http_events: &mut Vec<HttpEvent>) -> Result<String> {
    let bytes = tokio::fs::read(&req.local_path).await?;
    let client = ocis_client::build_http_client();

    let sanitised_url = {
        let mut u = req.remote_url.clone();
        u.set_query(None);
        u.set_fragment(None);
        u.to_string()
    };

    let mut builder = client
        .put(req.remote_url.as_str())
        .header("Content-Length", req.size.to_string())
        .body(bytes);

    if let Some(ref cs) = req.checksum {
        builder = builder.header("OC-Checksum", format!("SHA256:{cs}"));
    }

    let t0 = tokio::time::Instant::now();
    let resp = builder.send().await.map_err(|e| SyncError::Http {
        status: 0,
        message: e.to_string(),
    })?;

    let status = resp.status().as_u16();
    http_events.push(HttpEvent {
        method: "PUT".to_string(),
        url: sanitised_url,
        status,
        duration_ms: t0.elapsed().as_millis() as u64,
        bytes: req.size,
    });

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

async fn upload_tus(req: UploadRequest, http_events: &mut Vec<HttpEvent>) -> Result<String> {
    let client = ocis_client::build_http_client();

    let sanitised_url = {
        let mut u = req.remote_url.clone();
        u.set_query(None);
        u.set_fragment(None);
        u.to_string()
    };

    let t0 = tokio::time::Instant::now();
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

    let create_status = create_resp.status().as_u16();
    http_events.push(HttpEvent {
        method: "POST".to_string(),
        url: sanitised_url.clone(),
        status: create_status,
        duration_ms: t0.elapsed().as_millis() as u64,
        bytes: 0,
    });

    if create_status != 201 {
        return Err(SyncError::Http {
            status: create_status,
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

    let t1 = tokio::time::Instant::now();
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
    http_events.push(HttpEvent {
        method: "PATCH".to_string(),
        url: patch_url,
        status: patch_status,
        duration_ms: t1.elapsed().as_millis() as u64,
        bytes: req.size,
    });

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

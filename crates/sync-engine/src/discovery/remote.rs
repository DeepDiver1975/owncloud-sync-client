use camino::Utf8PathBuf;
use std::collections::VecDeque;
use std::time::SystemTime;
use url::Url;

use crate::error::{Result, SyncError};
use crate::report::HttpEvent;
use crate::types::RemoteEntry;

pub async fn discover_remote(
    space_root: &Url,
    bearer_token: &str,
    http_events: &mut Vec<HttpEvent>,
) -> Result<Vec<RemoteEntry>> {
    let client = ocis_client::build_http_client();
    let mut result = Vec::new();
    let mut queue = VecDeque::from([space_root.clone()]);

    while let Some(url) = queue.pop_front() {
        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:OC="http://owncloud.org/ns">
  <D:prop>
    <D:resourcetype/>
    <D:getlastmodified/>
    <D:getcontentlength/>
    <D:getetag/>
    <OC:fileid/>
    <OC:permissions/>
  </D:prop>
</D:propfind>"#;

        let t0 = tokio::time::Instant::now();
        let resp = client
            .request(
                reqwest::Method::from_bytes(b"PROPFIND").unwrap(),
                url.as_str(),
            )
            .bearer_auth(bearer_token)
            .header("Depth", "1")
            .header("Content-Type", "application/xml")
            .body(body)
            .send()
            .await
            .map_err(|e| SyncError::Http {
                status: 0,
                message: e.to_string(),
            })?;

        let status = resp.status().as_u16();
        let sanitised_url = sanitise_url(&url);

        if status != 207 {
            http_events.push(HttpEvent {
                method: "PROPFIND".to_string(),
                url: sanitised_url,
                status,
                duration_ms: t0.elapsed().as_millis() as u64,
                bytes: 0,
            });
            return Err(SyncError::Http {
                status,
                message: "PROPFIND failed".into(),
            });
        }

        let text = resp.text().await.map_err(|e| SyncError::Http {
            status: 0,
            message: e.to_string(),
        })?;

        http_events.push(HttpEvent {
            method: "PROPFIND".to_string(),
            url: sanitised_url,
            status,
            duration_ms: t0.elapsed().as_millis() as u64,
            bytes: text.len() as u64,
        });

        let (files, dirs) = parse_propfind(&text, space_root)?;
        result.extend(files);
        queue.extend(dirs);
    }

    Ok(result)
}

/// Strip query parameters from a URL, keeping only scheme + host + path.
fn sanitise_url(url: &Url) -> String {
    let mut u = url.clone();
    u.set_query(None);
    u.set_fragment(None);
    u.to_string()
}

fn parse_propfind(xml: &str, space_root: &Url) -> Result<(Vec<RemoteEntry>, Vec<Url>)> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);

    let mut files = Vec::new();
    let mut dirs = Vec::new();

    let mut href = String::new();
    let mut etag = String::new();
    let mut file_id = String::new();
    let mut size: u64 = 0;
    let mut is_collection = false;
    let mut in_href = false;
    let mut in_etag = false;
    let mut in_length = false;
    let mut in_fileid = false;
    let mut in_response = false;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = std::str::from_utf8(e.local_name().into_inner())
                    .unwrap_or("")
                    .to_owned();
                match name.as_str() {
                    "response" => {
                        in_response = true;
                        href.clear();
                        etag.clear();
                        file_id.clear();
                        size = 0;
                        is_collection = false;
                    }
                    "href" if in_response => in_href = true,
                    "getetag" => in_etag = true,
                    "getcontentlength" => in_length = true,
                    "fileid" => in_fileid = true,
                    "collection" => is_collection = true,
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = std::str::from_utf8(e.local_name().into_inner())
                    .unwrap_or("")
                    .to_owned();
                if name == "collection" {
                    is_collection = true;
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().into_owned();
                if in_href {
                    href = text.clone();
                }
                if in_etag {
                    etag = text.trim_matches('"').to_string();
                }
                if in_length {
                    size = text.parse().unwrap_or(0);
                }
                if in_fileid {
                    file_id = text.clone();
                }
                in_href = false;
                in_etag = false;
                in_length = false;
                in_fileid = false;
            }
            Ok(Event::End(ref e)) => {
                let name = std::str::from_utf8(e.local_name().into_inner()).unwrap_or("");
                if name == "response" && in_response {
                    in_response = false;
                    if href.is_empty() {
                        continue;
                    }

                    let root_path = space_root.path().trim_end_matches('/');
                    let rel = href
                        .strip_prefix(root_path)
                        .unwrap_or(&href)
                        .trim_start_matches('/');

                    if rel.is_empty() || href.trim_end_matches('/') == root_path {
                        continue;
                    }

                    if is_collection {
                        if let Ok(mut sub_url) = space_root.join(rel) {
                            if !sub_url.path().ends_with('/') {
                                sub_url.set_path(&format!("{}/", sub_url.path()));
                            }
                            dirs.push(sub_url);
                        }
                    } else {
                        let path = Utf8PathBuf::from(rel);
                        files.push(RemoteEntry {
                            path,
                            etag: etag.clone(),
                            mtime: SystemTime::UNIX_EPOCH,
                            size,
                            file_id: file_id.clone(),
                            permissions: 0,
                        });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(SyncError::Parse(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok((files, dirs))
}

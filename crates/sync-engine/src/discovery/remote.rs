use camino::Utf8PathBuf;
use percent_encoding::percent_decode_str;
use std::collections::{HashSet, VecDeque};
use std::time::SystemTime;
use url::Url;

use crate::error::{Result, SyncError};
use crate::report::HttpEvent;
use crate::types::RemoteEntry;

fn decode_href_path(s: &str) -> String {
    // TODO: add NFC/NFD Unicode normalization here per platform (macOS NFD vs Windows/Linux NFC)
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}

/// Fetch all remote entries under `space_root` using Depth:1 PROPFIND,
/// recursing into collections breadth-first. Appends one `HttpEvent` per
/// request to `http_events`.
pub async fn discover_remote(
    space_root: &Url,
    bearer_token: &str,
    http_events: &mut Vec<HttpEvent>,
) -> Result<Vec<RemoteEntry>> {
    let client = ocis_client::build_http_client();
    let mut result = Vec::new();
    let mut queue = VecDeque::from([space_root.clone()]);
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(space_root.path().trim_end_matches('/').to_string());
    let mut seen_entries: HashSet<String> = HashSet::new();

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
        let sanitised_url = crate::report::sanitise_url(&url);

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

        let (files, dirs) = parse_propfind(&text, space_root, &url)?;
        for entry in files {
            if seen_entries.insert(entry.path.as_str().to_owned()) {
                result.push(entry);
            }
        }
        for dir_url in dirs {
            let path_key = dir_url.path().trim_end_matches('/').to_string();
            if visited.insert(path_key) {
                queue.push_back(dir_url);
            }
        }
    }

    Ok(result)
}

fn parse_propfind(
    xml: &str,
    space_root: &Url,
    current_url: &Url,
) -> Result<(Vec<RemoteEntry>, Vec<Url>)> {
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
                    let current_path = current_url.path().trim_end_matches('/');
                    let rel_encoded = href
                        .strip_prefix(root_path)
                        .unwrap_or(&href)
                        .trim_start_matches('/');
                    let rel = decode_href_path(rel_encoded);
                    let rel = rel.as_str();

                    if rel.is_empty()
                        || href.trim_end_matches('/') == root_path
                        || href.trim_end_matches('/') == current_path
                    {
                        continue;
                    }

                    if is_collection {
                        // `rel` is the decoded path; build the sub-collection URL
                        // with per-segment percent-encoding (a raw `?`/`#` in a
                        // name must not be parsed as a query/fragment).
                        if let Ok(mut sub_url) =
                            crate::join_remote_path(space_root, rel.trim_end_matches('/'))
                        {
                            if !sub_url.path().ends_with('/') {
                                sub_url.set_path(&format!("{}/", sub_url.path()));
                            }
                            dirs.push(sub_url);
                        }
                        // reconciler needs dirs in the entry list, not only in the queue
                        let path = Utf8PathBuf::from(rel.trim_end_matches('/'));
                        files.push(RemoteEntry {
                            path,
                            etag: etag.clone(),
                            mtime: SystemTime::UNIX_EPOCH,
                            size: 0,
                            file_id: file_id.clone(),
                            permissions: 0,
                            is_dir: true,
                        });
                    } else {
                        let path = Utf8PathBuf::from(rel);
                        files.push(RemoteEntry {
                            path,
                            etag: etag.clone(),
                            mtime: SystemTime::UNIX_EPOCH,
                            size,
                            file_id: file_id.clone(),
                            permissions: 0,
                            is_dir: false,
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

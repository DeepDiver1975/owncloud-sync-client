// crates/ocis-client/src/webdav/propfind.rs
use chrono::{DateTime, Utc};
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::NsReader;

use crate::error::{OcisError, Result};

const NS_DAV: &[u8] = b"DAV:";
const NS_OC: &[u8] = b"http://owncloud.org/ns";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceType {
    File,
    Directory,
}

impl Default for ResourceType {
    fn default() -> Self {
        ResourceType::File
    }
}

#[derive(Debug, Clone)]
pub struct DavEntry {
    pub href: String,
    pub etag: Option<String>,
    pub last_modified: Option<DateTime<Utc>>,
    pub content_length: Option<u64>,
    pub resource_type: ResourceType,
    pub file_id: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct CurrentEntry {
    href: Option<String>,
    etag: Option<String>,
    last_modified: Option<DateTime<Utc>>,
    content_length: Option<u64>,
    resource_type: ResourceType,
    file_id: Option<String>,
    collecting: Collecting,
    /// Status text from current propstat block.
    propstat_status: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
enum Collecting {
    #[default]
    None,
    Href,
    Etag,
    LastModified,
    ContentLength,
    FileId,
    Status,
}

/// Parse a WebDAV PROPFIND multistatus XML body.
pub fn parse_propfind_response(xml: &str) -> Result<Vec<DavEntry>> {
    let mut reader = NsReader::from_str(xml);
    reader.trim_text(true);

    let mut entries: Vec<DavEntry> = Vec::new();
    let mut current: Option<CurrentEntry> = None;
    let mut in_response = false;
    // Whether the current propstat has HTTP 2xx status.
    let mut propstat_ok = true;

    let mut buf = Vec::new();

    loop {
        match reader.read_resolved_event_into(&mut buf) {
            Ok((ns, Event::Start(ref e))) => {
                let ns_bytes = match &ns {
                    ResolveResult::Bound(ns) => ns.as_ref(),
                    _ => b"",
                };
                let local = e.local_name();
                let local = local.as_ref();

                if ns_bytes == NS_DAV && local == b"response" {
                    in_response = true;
                    current = Some(CurrentEntry::default());
                    propstat_ok = true;
                    buf.clear();
                    continue;
                }

                if !in_response {
                    buf.clear();
                    continue;
                }

                if let Some(ref mut c) = current {
                    match (ns_bytes, local) {
                        (n, b"href") if n == NS_DAV => c.collecting = Collecting::Href,
                        (n, b"getetag") if n == NS_DAV => c.collecting = Collecting::Etag,
                        (n, b"getlastmodified") if n == NS_DAV => {
                            c.collecting = Collecting::LastModified
                        }
                        (n, b"getcontentlength") if n == NS_DAV => {
                            c.collecting = Collecting::ContentLength
                        }
                        (n, b"collection") if n == NS_DAV => {
                            c.resource_type = ResourceType::Directory
                        }
                        (n, b"fileid") if n == NS_OC => c.collecting = Collecting::FileId,
                        (n, b"status") if n == NS_DAV => {
                            c.collecting = Collecting::Status;
                            c.propstat_status.clear();
                        }
                        (n, b"propstat") if n == NS_DAV => {
                            // Reset propstat_ok at the start of each propstat block.
                            propstat_ok = true;
                        }
                        _ => {}
                    }
                }
            }

            Ok((ns, Event::Empty(ref e))) => {
                let ns_bytes = match &ns {
                    ResolveResult::Bound(ns) => ns.as_ref(),
                    _ => b"",
                };
                let local = e.local_name();
                let local = local.as_ref();

                if in_response && ns_bytes == NS_DAV && local == b"collection" {
                    if let Some(ref mut c) = current {
                        c.resource_type = ResourceType::Directory;
                    }
                }
            }

            Ok((_ns, Event::End(ref e))) => {
                let local = e.local_name();
                let local = local.as_ref();

                if local == b"response" {
                    if let Some(entry) = current.take() {
                        if let Some(href) = entry.href {
                            entries.push(DavEntry {
                                href,
                                etag: entry.etag,
                                last_modified: entry.last_modified,
                                content_length: entry.content_length,
                                resource_type: entry.resource_type,
                                file_id: entry.file_id,
                            });
                        }
                    }
                    in_response = false;
                    buf.clear();
                    continue;
                }

                if !in_response {
                    buf.clear();
                    continue;
                }

                // When a <D:status> element closes, check if it indicates non-2xx.
                if local == b"status" {
                    if let Some(ref c) = current {
                        // HTTP status lines: "HTTP/1.1 2XX ..." — check the status code part.
                        propstat_ok = c
                            .propstat_status
                            .split_whitespace()
                            .nth(1)
                            .and_then(|code| code.parse::<u16>().ok())
                            .map(|code| (200..300).contains(&code))
                            .unwrap_or(false);
                    }
                }

                if let Some(ref mut c) = current {
                    c.collecting = Collecting::None;
                }
            }

            Ok((_ns, Event::Text(ref e))) => {
                if !in_response {
                    buf.clear();
                    continue;
                }

                if let Some(ref mut c) = current {
                    let text = match e.unescape() {
                        Ok(t) => t.trim().to_string(),
                        Err(_) => {
                            buf.clear();
                            continue;
                        }
                    };
                    if text.is_empty() {
                        buf.clear();
                        continue;
                    }

                    match c.collecting {
                        Collecting::Href => c.href = Some(text),
                        Collecting::Etag if propstat_ok => {
                            c.etag = Some(text.trim_matches('"').to_string())
                        }
                        Collecting::LastModified if propstat_ok => {
                            if let Ok(dt) = DateTime::parse_from_rfc2822(&text) {
                                c.last_modified = Some(dt.with_timezone(&Utc));
                            }
                        }
                        Collecting::ContentLength if propstat_ok => {
                            if let Ok(n) = text.parse::<u64>() {
                                c.content_length = Some(n);
                            }
                        }
                        Collecting::FileId if propstat_ok => c.file_id = Some(text),
                        Collecting::Status => c.propstat_status = text,
                        _ => {}
                    }
                }
            }

            Ok((_ns, Event::Eof)) => break,
            Err(e) => return Err(OcisError::Parse(format!("XML parse error: {e}"))),
            _ => {}
        }

        buf.clear();
    }

    Ok(entries)
}

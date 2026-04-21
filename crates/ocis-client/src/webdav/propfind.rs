// crates/ocis-client/src/webdav/propfind.rs
use chrono::{DateTime, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;

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
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);

    let mut entries: Vec<DavEntry> = Vec::new();
    let mut current: Option<CurrentEntry> = None;
    let mut in_response = false;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name_bytes: Vec<u8> = e.name().as_ref().to_vec();
                let (ns, local) = split_name_owned(&name_bytes);

                if ns == NS_DAV && local == b"response" {
                    in_response = true;
                    current = Some(CurrentEntry::default());
                    buf.clear();
                    continue;
                }

                if !in_response {
                    buf.clear();
                    continue;
                }

                if let Some(ref mut c) = current {
                    match (ns, local) {
                        (n, l) if n == NS_DAV && l == b"href" => c.collecting = Collecting::Href,
                        (n, l) if n == NS_DAV && l == b"getetag" => c.collecting = Collecting::Etag,
                        (n, l) if n == NS_DAV && l == b"getlastmodified" => c.collecting = Collecting::LastModified,
                        (n, l) if n == NS_DAV && l == b"getcontentlength" => c.collecting = Collecting::ContentLength,
                        (n, l) if n == NS_DAV && l == b"collection" => c.resource_type = ResourceType::Directory,
                        (n, l) if n == NS_OC && l == b"fileid" => c.collecting = Collecting::FileId,
                        (n, l) if n == NS_DAV && l == b"status" => c.collecting = Collecting::Status,
                        _ => {}
                    }
                }
            }

            Ok(Event::Empty(ref e)) => {
                let name_bytes: Vec<u8> = e.name().as_ref().to_vec();
                let (ns, local) = split_name_owned(&name_bytes);
                if in_response && ns == NS_DAV && local == b"collection" {
                    if let Some(ref mut c) = current {
                        c.resource_type = ResourceType::Directory;
                    }
                }
            }

            Ok(Event::End(ref e)) => {
                let name_bytes: Vec<u8> = e.name().as_ref().to_vec();
                let (ns, local) = split_name_owned(&name_bytes);

                if ns == NS_DAV && local == b"response" {
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

                if let Some(ref mut c) = current {
                    c.collecting = Collecting::None;
                }
            }

            Ok(Event::Text(ref e)) => {
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
                        Collecting::Etag => c.etag = Some(text.trim_matches('"').to_string()),
                        Collecting::LastModified => {
                            if let Ok(dt) = DateTime::parse_from_rfc2822(&text) {
                                c.last_modified = Some(dt.with_timezone(&Utc));
                            }
                        }
                        Collecting::ContentLength => {
                            if let Ok(n) = text.parse::<u64>() {
                                c.content_length = Some(n);
                            }
                        }
                        Collecting::FileId => c.file_id = Some(text),
                        _ => {}
                    }
                }
            }

            Ok(Event::Eof) => break,
            Err(e) => return Err(OcisError::Parse(format!("XML parse error: {e}"))),
            _ => {}
        }

        buf.clear();
    }

    Ok(entries)
}

/// Split a Clark-notation element name `{namespace}local` into `(namespace, local)`.
/// Returns `(b"", name)` if there is no namespace.
fn split_name_owned(name: &[u8]) -> (&[u8], &[u8]) {
    if let Some(pos) = name.iter().position(|&b| b == b'}') {
        (&name[1..pos], &name[pos + 1..])
    } else {
        (b"", name)
    }
}

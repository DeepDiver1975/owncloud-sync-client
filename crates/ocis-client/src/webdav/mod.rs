// crates/ocis-client/src/webdav/mod.rs
pub mod propfind;

pub use propfind::{parse_propfind_response, DavEntry, ResourceType};

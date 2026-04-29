// crates/ocis-client/src/lib.rs
pub mod auth;
pub mod error;
pub mod graph;
pub mod tus;
pub mod webdav;

pub use error::{OcisError, Result};
pub use graph::{webdav_url_for_space, GraphClient, Space, SpaceQuota};
pub use tus::{TusClient, TusUpload};

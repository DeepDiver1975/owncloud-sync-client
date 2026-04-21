// crates/ocis-client/src/lib.rs
pub mod auth;
pub mod error;
pub mod graph;
pub mod tus;
pub mod webdav;

pub use error::{OcisError, Result};

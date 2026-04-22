pub mod error;
pub mod messages;
// xpc contains macOS-only code but is always compiled so the crate builds on Linux.
pub mod xpc;

pub use error::VfsMacOsError;
pub use messages::{XpcCommand, XpcReply};

#[cfg(target_os = "macos")]
mod vfs_impl;
#[cfg(target_os = "macos")]
pub use vfs_impl::VfsMacOs;

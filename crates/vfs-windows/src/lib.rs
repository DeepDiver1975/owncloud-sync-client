pub mod error;
// util is pure Rust — not cfg-gated so its unit tests run on all platforms.
pub mod util;

#[cfg(target_os = "windows")]
pub mod callback;
#[cfg(target_os = "windows")]
pub mod hydration;
#[cfg(target_os = "windows")]
pub mod pin;
#[cfg(target_os = "windows")]
pub mod placeholder;
#[cfg(target_os = "windows")]
pub mod registration;
#[cfg(target_os = "windows")]
mod vfs_impl;

#[cfg(target_os = "windows")]
pub use callback::{HydrationCallbackContext, HydrationRequest};
#[cfg(target_os = "windows")]
pub use vfs_impl::VfsWindows;

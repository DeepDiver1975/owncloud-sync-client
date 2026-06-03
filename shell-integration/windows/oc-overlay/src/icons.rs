//! icons.rs — Embed overlay icon PNGs at compile time and create HICONs.
//!
//! Each PNG is a 16x16 RGBA image embedded via include_bytes!. On first use
//! each icon is loaded from the byte slice using CreateIconFromResourceEx and
//! cached in a static OnceLock. Placeholder PNGs (minimal 1×1 pixel) are
//! used during development; replace with real artwork before shipping.

use std::sync::OnceLock;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, HICON, LR_DEFAULTCOLOR};

// Minimal valid 1×1 RGBA PNG — 67 bytes.
// Replace these with real 16×16 artwork for production builds.
const PLACEHOLDER_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG signature
    0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // width=1, height=1
    0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // bit depth=8, color=RGB
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, // IDAT chunk
    0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, // compressed pixel data
    0x00, 0x00, 0x02, 0x00, 0x01, 0xe2, 0x21, 0xbc, 0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, // IEND chunk
    0x44, 0xae, 0x42, 0x60, 0x82,
];

// Use placeholder PNGs for now; replace with include_bytes! for real artwork:
//   const ICON_OK_PNG: &[u8] = include_bytes!("../icons/ok.png");
const ICON_OK_PNG: &[u8] = PLACEHOLDER_PNG;
const ICON_SYNC_PNG: &[u8] = PLACEHOLDER_PNG;
const ICON_WARNING_PNG: &[u8] = PLACEHOLDER_PNG;
const ICON_ERROR_PNG: &[u8] = PLACEHOLDER_PNG;
const ICON_EXCLUDED_PNG: &[u8] = PLACEHOLDER_PNG;

/// Load a PNG byte slice as a Win32 HICON using CreateIconFromResourceEx.
///
/// # Safety
/// `bytes` must remain valid for the lifetime of the returned HICON. Since
/// we only call this with `'static` slices the invariant is always satisfied.
fn load_icon_from_bytes(bytes: &'static [u8]) -> Option<HICON> {
    // SAFETY: `bytes` is a 'static slice pointing to embedded PNG data.
    // CreateIconFromResourceEx reads from this pointer synchronously and
    // does not retain a reference after the call returns.
    let icon = unsafe {
        CreateIconFromResourceEx(
            bytes,
            true,        // fIcon = TRUE
            0x0003_0000, // dwVer = 0x00030000
            0,           // cxDesired = 0 → natural width
            0,           // cyDesired = 0 → natural height
            LR_DEFAULTCOLOR,
        )
        .ok()?
    };
    Some(icon)
}

pub fn icon_ok() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_OK_PNG))
}

pub fn icon_sync() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_SYNC_PNG))
}

pub fn icon_warning() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_WARNING_PNG))
}

pub fn icon_error() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_ERROR_PNG))
}

pub fn icon_excluded() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_EXCLUDED_PNG))
}

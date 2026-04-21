# Plan 3: VFS Windows — CloudFiles API (CfAPI)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `vfs-windows` — a Rust crate wrapping Windows CloudFiles API (CfAPI) that satisfies the `vfs-core::Vfs` trait, providing native virtual file support on Windows.

**Architecture:** Uses `windows-rs` crate for CfAPI bindings. Registers a sync root per folder via `CfRegisterSyncRoot`. Handles OS-initiated hydration via a callback thread dispatching to `tokio::task::spawn_blocking`. All `Vfs` trait methods delegate to CfAPI functions.

**Tech Stack:** Rust 2021, windows-rs (0.52+), tokio, camino, thiserror. Only compiles on Windows (cfg target_os). Depends on vfs-core (Plan 2).

---

## Task 1: Cargo.toml + error.rs

- [ ] Add `crates/vfs-windows` to the workspace members in the root `Cargo.toml`:

```toml
members = [
    "crates/sync-db",
    "crates/ocis-client",
    "crates/vfs-core",
    "crates/vfs-off",
    "crates/vfs-windows",
    "crates/sync-engine",
]
```

Also add the `windows` crate as a workspace dependency:

```toml
[workspace.dependencies]
windows = { version = "0.52", features = [
    "Storage_Provider_CloudFiles",
    "Win32_Storage_CloudFilters",
    "Win32_System_Com",
    "Win32_Foundation",
    "Win32_Storage_FileSystem",
    "Win32_System_IO",
] }
```

- [ ] Create directories:

```bash
mkdir -p crates/vfs-windows/src
mkdir -p crates/vfs-windows/tests
```

- [ ] Write the failing test first. Create `crates/vfs-windows/tests/error_convert.rs`:

```rust
// tests/error_convert.rs
// Pure logic test — no Windows filesystem required.
#[cfg(target_os = "windows")]
mod tests {
    use vfs_core::VfsError;
    use vfs_windows::error::VfsWindowsError;

    #[test]
    fn vfs_windows_error_display_not_supported() {
        let e = VfsWindowsError::NotSupported("test operation".into());
        assert!(e.to_string().contains("test operation"));
    }

    #[test]
    fn vfs_windows_error_into_vfs_error() {
        let e = VfsWindowsError::NotSupported("hydrate".into());
        let vfs_err: VfsError = e.into();
        assert!(vfs_err.to_string().contains("hydrate"));
    }

    #[test]
    fn vfs_windows_error_path_not_found() {
        use camino::Utf8PathBuf;
        let e = VfsWindowsError::PathNotFound(Utf8PathBuf::from("C:\\foo\\bar.txt"));
        let vfs_err: VfsError = e.into();
        assert!(matches!(vfs_err, VfsError::NotFound { .. }));
    }

    #[test]
    fn vfs_windows_error_backend_string() {
        let e = VfsWindowsError::Backend("CfRegisterSyncRoot failed".into());
        let vfs_err: VfsError = e.into();
        assert!(matches!(vfs_err, VfsError::Backend(_)));
    }
}

// On non-Windows, the test file must still compile but contains no tests.
#[cfg(not(target_os = "windows"))]
fn main() {}
```

- [ ] Run (expect failure — crate does not exist yet):

```bash
cargo test -p vfs-windows --test error_convert 2>&1 | head -20
# Expected: error[E0432]: unresolved import `vfs_windows`
```

- [ ] Create `crates/vfs-windows/Cargo.toml`:

```toml
[package]
name = "vfs-windows"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

[target.'cfg(target_os = "windows")'.dependencies]
windows = { workspace = true }

[dependencies]
vfs-core  = { path = "../vfs-core" }
camino    = { workspace = true }
thiserror = { workspace = true }
tokio     = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
tempfile = { workspace = true }
```

- [ ] Create `crates/vfs-windows/src/lib.rs`:

```rust
//! vfs-windows — Windows CloudFiles API (CfAPI) VFS implementation.
//!
//! This crate only provides a real implementation when compiled on Windows
//! (`target_os = "windows"`).  On other platforms the public surface still
//! exists but every method returns [`VfsError::NotSupported`].

pub mod error;

#[cfg(target_os = "windows")]
mod registration;
#[cfg(target_os = "windows")]
mod placeholder;
#[cfg(target_os = "windows")]
mod hydration;
#[cfg(target_os = "windows")]
mod pin;
#[cfg(target_os = "windows")]
mod callback;

#[cfg(target_os = "windows")]
mod vfs_impl;

#[cfg(target_os = "windows")]
pub use vfs_impl::VfsWindows;

#[cfg(target_os = "windows")]
pub use callback::{HydrationCallbackContext, HydrationRequest};
```

- [ ] Create `crates/vfs-windows/src/error.rs`:

```rust
//! Error type for the vfs-windows crate.

use camino::Utf8PathBuf;
use thiserror::Error;
use vfs_core::{VfsError, VfsStatus};

/// All errors that can be produced by vfs-windows CfAPI operations.
#[derive(Debug, Error)]
pub enum VfsWindowsError {
    /// A CfAPI or Win32 call returned a non-zero HRESULT.
    #[cfg(target_os = "windows")]
    #[error("CfAPI error: {0}")]
    CfApi(#[from] windows::core::Error),

    /// A path was not found on the local filesystem.
    #[error("Path not found: {0}")]
    PathNotFound(Utf8PathBuf),

    /// A CfAPI operation is not supported on this build or configuration.
    #[error("Operation not supported: {0}")]
    NotSupported(String),

    /// A low-level I/O error not originating from a CfAPI call.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A string conversion error (e.g. path to wide string).
    #[error("String conversion error: {0}")]
    StringConversion(String),

    /// A generic backend error with a descriptive message.
    #[error("VFS backend error: {0}")]
    Backend(String),
}

impl From<VfsWindowsError> for VfsError {
    fn from(e: VfsWindowsError) -> Self {
        match e {
            VfsWindowsError::PathNotFound(path) => VfsError::NotFound { path },
            VfsWindowsError::NotSupported(_) => VfsError::NotSupported,
            VfsWindowsError::Io(io) => VfsError::Io(io),
            other => VfsError::Backend(other.to_string()),
        }
    }
}

/// Convenience alias used throughout this crate.
pub type Result<T, E = VfsWindowsError> = std::result::Result<T, E>;
```

- [ ] Run (expect pass on Windows; on Linux/macOS the `#[cfg]` gates suppress the Windows-specific variants, so at minimum the non-cfg variants and the `From` impl compile):

```bash
cargo test -p vfs-windows --test error_convert 2>&1
# On Windows: 4 tests pass
# On Linux:   compiles, 0 tests collected (all gated behind cfg(target_os="windows"))
```

- [ ] Commit:

```bash
git add crates/vfs-windows/ Cargo.toml
git commit -m "feat(vfs-windows): scaffold crate, Cargo.toml, VfsWindowsError with From<VfsError>"
```

---

## Task 2: registration.rs

- [ ] Write the failing test first. Create `crates/vfs-windows/tests/registration.rs`:

```rust
// tests/registration.rs
#[cfg(target_os = "windows")]
mod tests {
    use camino::Utf8Path;
    use vfs_windows::error::Result;
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    /// Registers and immediately unregisters a sync root on a temp NTFS directory.
    ///
    /// Requires a real Windows NTFS volume and administrator privileges (or
    /// Developer Mode enabled) to run.  Marked `#[ignore]` for CI.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn register_and_unregister_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0")
            .expect("register_sync_root should succeed");

        unregister_sync_root(root).expect("unregister_sync_root should succeed");
    }

    /// Trying to unregister a path that was never registered must return an error
    /// rather than panicking.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn unregister_nonexistent_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();
        // No prior registration — CfUnregisterSyncRoot should return an HRESULT error.
        let result = unregister_sync_root(root);
        assert!(result.is_err(), "should fail for unregistered path");
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}
```

- [ ] Run (expect failure — module missing):

```bash
cargo test -p vfs-windows --test registration 2>&1 | head -20
# Expected: error[E0432]: unresolved import or module not found
```

- [ ] Create `crates/vfs-windows/src/registration.rs`:

```rust
//! Sync root registration and unregistration via CfRegisterSyncRoot /
//! CfUnregisterSyncRoot.

use camino::Utf8Path;

use crate::error::{Result, VfsWindowsError};
use crate::util::to_wide_null;

use windows::core::HSTRING;
use windows::Win32::Storage::CloudFilters::{
    CfRegisterSyncRoot, CfUnregisterSyncRoot,
    CF_HYDRATION_POLICY, CF_HYDRATION_POLICY_MODIFIER,
    CF_HYDRATION_POLICY_PARTIAL,
    CF_POPULATION_POLICY, CF_POPULATION_POLICY_MODIFIER,
    CF_POPULATION_POLICY_PARTIAL,
    CF_REGISTER_FLAG_NONE,
    CF_SYNC_PROVIDER_INFO,
    CF_SYNC_REGISTRATION,
};
use windows::Win32::Foundation::HANDLE;

/// Register `path` as a CfAPI sync root.
///
/// `provider_name` and `provider_version` appear in Windows Settings under
/// "Cloud-delivered protection" and similar UI surfaces.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if `CfRegisterSyncRoot` fails (e.g. the
/// path does not exist, is not on NTFS, or the caller lacks sufficient
/// privileges).
pub fn register_sync_root(
    path: &Utf8Path,
    provider_name: &str,
    provider_version: &str,
) -> Result<()> {
    // Safety: HSTRING takes ownership of a heap-allocated wide string;
    // the conversion is infallible for valid UTF-8 inputs.
    let path_wide = HSTRING::from(path.as_str());
    let name_wide = HSTRING::from(provider_name);
    let version_wide = HSTRING::from(provider_version);

    let registration = CF_SYNC_REGISTRATION {
        StructSize: std::mem::size_of::<CF_SYNC_REGISTRATION>() as u32,
        ProviderName: windows::core::PCWSTR(name_wide.as_ptr()),
        ProviderVersion: windows::core::PCWSTR(version_wide.as_ptr()),
        SyncRootIdentity: std::ptr::null(),
        SyncRootIdentityLength: 0,
        FileIdentity: std::ptr::null(),
        FileIdentityLength: 0,
        ProviderId: windows::core::GUID::zeroed(),
    };

    let hydration_policy = CF_HYDRATION_POLICY {
        Primary: CF_HYDRATION_POLICY_PARTIAL,
        Modifier: CF_HYDRATION_POLICY_MODIFIER(0),
    };

    let population_policy = CF_POPULATION_POLICY {
        Primary: CF_POPULATION_POLICY_PARTIAL,
        Modifier: CF_POPULATION_POLICY_MODIFIER(0),
    };

    // Safety: CfRegisterSyncRoot is an FFI call; all pointer arguments have
    // been constructed from valid Rust values immediately above.
    unsafe {
        CfRegisterSyncRoot(
            &path_wide,
            &registration,
            &hydration_policy,
            &population_policy,
            CF_REGISTER_FLAG_NONE,
        )
    }
    .map_err(VfsWindowsError::CfApi)?;

    Ok(())
}

/// Unregister the CfAPI sync root at `path`.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the path was never registered or the
/// call otherwise fails.
pub fn unregister_sync_root(path: &Utf8Path) -> Result<()> {
    let path_wide = HSTRING::from(path.as_str());

    // Safety: path_wide is a valid wide string; no other pointer arguments.
    unsafe { CfUnregisterSyncRoot(&path_wide) }.map_err(VfsWindowsError::CfApi)?;

    Ok(())
}
```

- [ ] Add a `util` module for shared helpers. Create `crates/vfs-windows/src/util.rs`:

```rust
//! Internal helpers shared across vfs-windows sub-modules.

/// Convert a UTF-8 string to a null-terminated wide (UTF-16) `Vec<u16>`.
///
/// The trailing `\0` is appended so the result can be passed to Win32 APIs
/// expecting a `LPCWSTR`.
pub fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0u16)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_wide_null_ascii() {
        let wide = to_wide_null("hello");
        assert_eq!(wide, &[b'h' as u16, b'e' as u16, b'l' as u16, b'l' as u16, b'o' as u16, 0]);
    }

    #[test]
    fn to_wide_null_empty() {
        let wide = to_wide_null("");
        assert_eq!(wide, &[0u16]);
    }

    #[test]
    fn to_wide_null_unicode() {
        // "ñ" is U+00F1, fits in one UTF-16 code unit.
        let wide = to_wide_null("ñ");
        assert_eq!(wide[0], 0x00F1u16);
        assert_eq!(*wide.last().unwrap(), 0u16);
    }
}
```

- [ ] Update `crates/vfs-windows/src/lib.rs` to expose `registration` publicly (for tests) and declare `util`:

```rust
//! vfs-windows — Windows CloudFiles API (CfAPI) VFS implementation.

pub mod error;

#[cfg(target_os = "windows")]
mod util;
#[cfg(target_os = "windows")]
pub mod registration;
#[cfg(target_os = "windows")]
mod placeholder;
#[cfg(target_os = "windows")]
mod hydration;
#[cfg(target_os = "windows")]
mod pin;
#[cfg(target_os = "windows")]
mod callback;
#[cfg(target_os = "windows")]
mod vfs_impl;

#[cfg(target_os = "windows")]
pub use vfs_impl::VfsWindows;
#[cfg(target_os = "windows")]
pub use callback::{HydrationCallbackContext, HydrationRequest};
```

- [ ] Add stubs so the crate compiles end-to-end. Each file should contain at minimum a `// TODO` comment and any necessary `use` statements:

`crates/vfs-windows/src/placeholder.rs` — `// TODO: implemented in Task 3`  
`crates/vfs-windows/src/hydration.rs` — `// TODO: implemented in Task 4`  
`crates/vfs-windows/src/pin.rs` — `// TODO: implemented in Task 5`  
`crates/vfs-windows/src/callback.rs` — `// TODO: implemented in Task 6`  
`crates/vfs-windows/src/vfs_impl.rs` — `// TODO: implemented in Task 7`  

- [ ] The `util` tests are pure Rust and run on all platforms. Verify:

```bash
cargo test -p vfs-windows 2>&1
# On Linux: util tests (to_wide_null_*) pass; registration tests skipped (cfg gate)
# On Windows: util tests pass; registration tests are #[ignore] (run with --ignored)
```

- [ ] Run registration integration tests on Windows:

```bash
# Windows only:
cargo test -p vfs-windows --test registration -- --ignored 2>&1
# Expected: register_and_unregister_roundtrip ... ok
#           unregister_nonexistent_returns_error ... ok
```

- [ ] Commit:

```bash
git add crates/vfs-windows/src/registration.rs \
        crates/vfs-windows/src/util.rs \
        crates/vfs-windows/src/lib.rs \
        crates/vfs-windows/src/placeholder.rs \
        crates/vfs-windows/src/hydration.rs \
        crates/vfs-windows/src/pin.rs \
        crates/vfs-windows/src/callback.rs \
        crates/vfs-windows/src/vfs_impl.rs
git commit -m "feat(vfs-windows): registration.rs — register/unregister sync root via CfAPI"
```

---

## Task 3: placeholder.rs

- [ ] Write the failing test first. Create `crates/vfs-windows/tests/placeholder.rs`:

```rust
// tests/placeholder.rs
#[cfg(target_os = "windows")]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use std::time::SystemTime;
    use vfs_core::VfsFileItem;
    use vfs_windows::placeholder::{create_placeholder, update_placeholder};
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    fn make_item(name: &str, size: u64, file_id: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size,
            etag: "etag-test".into(),
            file_id: file_id.into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// Creates a sync root, creates a placeholder file, then verifies the file
    /// appears on disk with the correct size (0 bytes — it is a placeholder).
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn create_placeholder_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();

        let item = make_item("hello.txt", 1024, "file-id-001");
        create_placeholder(root, &item).expect("create_placeholder should succeed");

        let file_path = dir.path().join("hello.txt");
        assert!(file_path.exists(), "placeholder file should exist on disk");
        // Placeholder is 0 bytes on disk until hydrated.
        assert_eq!(file_path.metadata().unwrap().len(), 0);

        unregister_sync_root(root).unwrap();
    }

    /// Creates a placeholder then updates its metadata (new size + etag).
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn update_placeholder_changes_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();

        let item = make_item("update_me.txt", 512, "file-id-002");
        create_placeholder(root, &item).unwrap();

        let updated = make_item("update_me.txt", 2048, "file-id-002");
        let full_path = root.join("update_me.txt");
        update_placeholder(&full_path, &updated)
            .expect("update_placeholder should succeed");

        unregister_sync_root(root).unwrap();
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}
```

- [ ] Run (expect failure — module missing):

```bash
cargo test -p vfs-windows --test placeholder 2>&1 | head -20
```

- [ ] Replace `crates/vfs-windows/src/placeholder.rs` with:

```rust
//! Placeholder creation and update via CfCreatePlaceholders / CfUpdatePlaceholder.

use camino::Utf8Path;
use std::mem;
use std::time::SystemTime;

use windows::core::HSTRING;
use windows::Win32::Storage::CloudFilters::{
    CfCreatePlaceholders,
    CfUpdatePlaceholder,
    CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
    CF_PLACEHOLDER_CREATE_INFO,
    CF_PLACEHOLDER_STANDARD_INFO,
    CF_SET_PIN_FLAG_NONE,
    CF_UPDATE_FLAG_MARK_IN_SYNC,
    CF_UPDATE_FLAG_NONE,
};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};

use vfs_core::VfsFileItem;

use crate::error::{Result, VfsWindowsError};

/// Convert a [`SystemTime`] to a Win32 [`FILETIME`].
///
/// FILETIME is 100-nanosecond intervals since 1601-01-01 00:00:00 UTC.
fn system_time_to_filetime(t: SystemTime) -> FILETIME {
    // Duration from 1601-01-01 to 1970-01-01 in 100-ns intervals.
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    let since_unix = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let intervals = EPOCH_DIFF_100NS
        + since_unix.as_secs() * 10_000_000
        + since_unix.subsec_nanos() as u64 / 100;
    FILETIME {
        dwLowDateTime: (intervals & 0xFFFF_FFFF) as u32,
        dwHighDateTime: (intervals >> 32) as u32,
    }
}

/// Create a dehydrated placeholder for `item` inside `root`.
///
/// `item.file_id` is stored as the opaque file identity blob so that the
/// hydration callback can identify which remote file to fetch.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if `CfCreatePlaceholders` fails.
pub fn create_placeholder(root: &Utf8Path, item: &VfsFileItem) -> Result<()> {
    // Build the file identity from the UTF-8 file_id bytes.
    let identity = item.file_id.as_bytes();

    // The relative filename within the sync root (leaf name only).
    let filename = item
        .path
        .file_name()
        .ok_or_else(|| VfsWindowsError::StringConversion(
            format!("path has no filename: {}", item.path),
        ))?;
    let filename_wide = HSTRING::from(filename);

    let last_write = system_time_to_filetime(item.last_modified);

    let mut create_info = CF_PLACEHOLDER_CREATE_INFO {
        RelativeFileName: windows::core::PCWSTR(filename_wide.as_ptr()),
        FsMetadata: windows::Win32::Storage::CloudFilters::CF_FS_METADATA {
            FileSize: item.size as i64,
            BasicInfo: windows::Win32::Storage::FileSystem::FILE_BASIC_INFO {
                LastWriteTime: last_write.into(),
                ChangeTime: last_write.into(),
                CreationTime: last_write.into(),
                LastAccessTime: last_write.into(),
                FileAttributes: 0,
            },
        },
        FileIdentity: identity.as_ptr() as *const _,
        FileIdentityLength: identity.len() as u32,
        FileIdentityExtensionLength: 0,
        FileIdentityExtension: std::ptr::null(),
        CreateFlags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
        Result: windows::Win32::Foundation::S_OK,
        CreateUsn: 0,
    };

    let root_wide = HSTRING::from(root.as_str());

    // Safety: all pointer fields in create_info reference data that lives at
    // least as long as this stack frame; CfCreatePlaceholders does not retain
    // them after the call returns.
    unsafe {
        CfCreatePlaceholders(
            windows::core::PCWSTR(root_wide.as_ptr()),
            &mut create_info,
            1,
            CF_CREATE_FLAG_NONE,
            std::ptr::null_mut(),
        )
    }
    .map_err(VfsWindowsError::CfApi)?;

    Ok(())
}

/// Update the metadata of an existing placeholder at `path`.
///
/// Opens the file with `CreateFileW`, calls `CfUpdatePlaceholder`, then closes
/// the handle.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the file cannot be opened or the
/// update fails.
pub fn update_placeholder(path: &Utf8Path, item: &VfsFileItem) -> Result<()> {
    let path_wide = HSTRING::from(path.as_str());
    let identity = item.file_id.as_bytes();
    let last_write = system_time_to_filetime(item.last_modified);

    // Safety: CreateFileW is an FFI call; path_wide is a valid null-terminated
    // wide string.  The returned HANDLE is checked immediately.
    let handle = unsafe {
        CreateFileW(
            windows::core::PCWSTR(path_wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(VfsWindowsError::CfApi)?;

    if handle == INVALID_HANDLE_VALUE {
        return Err(VfsWindowsError::PathNotFound(path.to_owned()));
    }

    let fs_metadata = windows::Win32::Storage::CloudFilters::CF_FS_METADATA {
        FileSize: item.size as i64,
        BasicInfo: windows::Win32::Storage::FileSystem::FILE_BASIC_INFO {
            LastWriteTime: last_write.into(),
            ChangeTime: last_write.into(),
            CreationTime: last_write.into(),
            LastAccessTime: last_write.into(),
            FileAttributes: 0,
        },
    };

    // Safety: handle is valid (checked above); all pointer arguments are valid
    // Rust references that outlive this call.
    let result = unsafe {
        CfUpdatePlaceholder(
            handle,
            &fs_metadata,
            identity.as_ptr() as *const _,
            identity.len() as u32,
            std::ptr::null(),
            0,
            CF_UPDATE_FLAG_MARK_IN_SYNC,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    // Always close handle before returning.
    // Safety: handle is valid; CloseHandle takes ownership.
    unsafe { CloseHandle(handle) };

    result.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}
```

- [ ] Update `VfsFileItem` in `vfs-core` to add a `last_modified: SystemTime` field (if not already present from Plan 2). This is needed by placeholder creation:

```rust
// In crates/vfs-core/src/lib.rs — update VfsFileItem:
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsFileItem {
    pub path: Utf8PathBuf,
    pub size: u64,
    pub etag: String,
    pub file_id: String,
    /// Last-modified time used to stamp placeholder metadata.
    #[serde(with = "serde_millis")]
    pub last_modified: std::time::SystemTime,
}
```

Add `serde-millis` to `vfs-core/Cargo.toml`:

```toml
serde-millis = "0.1"
```

And to workspace dependencies in the root `Cargo.toml`:

```toml
serde-millis = "0.1"
```

- [ ] Run (placeholder tests are all `#[ignore]`; verify the crate compiles cleanly on all platforms):

```bash
cargo build -p vfs-windows 2>&1
# Expected: compiles without errors
cargo test -p vfs-windows --test placeholder 2>&1
# On Linux: 0 tests collected (all cfg-gated)
```

- [ ] Run on Windows (with `--ignored`):

```bash
# Windows only:
cargo test -p vfs-windows --test placeholder -- --ignored 2>&1
# Expected:
# test tests::create_placeholder_creates_file ... ok
# test tests::update_placeholder_changes_metadata ... ok
```

- [ ] Commit:

```bash
git add crates/vfs-windows/src/placeholder.rs crates/vfs-core/src/lib.rs crates/vfs-core/Cargo.toml Cargo.toml
git commit -m "feat(vfs-windows): placeholder.rs — CfCreatePlaceholders + CfUpdatePlaceholder"
```

---

## Task 4: hydration.rs

- [ ] Write the failing test first. Create `crates/vfs-windows/tests/hydration.rs`:

```rust
// tests/hydration.rs
#[cfg(target_os = "windows")]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use std::time::SystemTime;
    use vfs_core::{VfsFileItem, VfsStatus};
    use vfs_windows::hydration::{dehydrate, hydrate, is_virtual, status};
    use vfs_windows::placeholder::create_placeholder;
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    fn make_item(name: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size: 1024,
            etag: "etag-hydration".into(),
            file_id: "fid-hydration".into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// After creating a placeholder, is_virtual() must return true.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn placeholder_is_virtual() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();
        create_placeholder(root, &make_item("v.txt")).unwrap();

        let file = root.join("v.txt");
        assert!(
            is_virtual(&file).expect("is_virtual should succeed"),
            "newly created placeholder must be virtual"
        );

        unregister_sync_root(root).unwrap();
    }

    /// Status of a fresh placeholder must be VfsStatus::Placeholder.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn status_of_placeholder_is_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();
        create_placeholder(root, &make_item("s.txt")).unwrap();

        let file = root.join("s.txt");
        let s = status(&file).expect("status should succeed");
        assert_eq!(s, VfsStatus::Placeholder);

        unregister_sync_root(root).unwrap();
    }

    /// hydrate() and dehydrate() must not panic on a valid placeholder.
    /// We cannot verify actual content download here (requires real sync engine),
    /// but the CfAPI calls must succeed without returning an error.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn hydrate_dehydrate_do_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();
        create_placeholder(root, &make_item("hd.txt")).unwrap();
        let file = root.join("hd.txt");

        // hydrate() will trigger a FETCH_DATA callback; since no real callback
        // handler is running, it may return a timeout or "no provider" error.
        // We only verify it doesn't panic.
        let _ = hydrate(&file);

        // dehydrate() on an already-dehydrated placeholder should succeed.
        let result = dehydrate(&file);
        assert!(result.is_ok(), "dehydrate on placeholder: {:?}", result);

        unregister_sync_root(root).unwrap();
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}
```

- [ ] Run (expect failure — module missing):

```bash
cargo test -p vfs-windows --test hydration 2>&1 | head -20
```

- [ ] Replace `crates/vfs-windows/src/hydration.rs` with:

```rust
//! Hydration, dehydration, virtual-status query, and VfsStatus mapping.

use camino::Utf8Path;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::CloudFilters::{
    CfDehydratePlaceholder,
    CfGetPlaceholderStateFromFileInfo,
    CfHydratePlaceholder,
    CF_DEHYDRATE_FLAG_NONE,
    CF_HYDRATE_FLAG_NONE,
    CF_PLACEHOLDER_STATE_IN_SYNC,
    CF_PLACEHOLDER_STATE_NO_STATES,
    CF_PLACEHOLDER_STATE_PARTIAL,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::WindowsProgramming::FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;

use vfs_core::VfsStatus;

use crate::error::{Result, VfsWindowsError};

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Open a file handle suitable for CfAPI operations.
///
/// # Safety
///
/// The caller is responsible for closing the returned handle with
/// `CloseHandle`.  The handle is always valid when this function returns `Ok`.
unsafe fn open_for_cf(path: &Utf8Path, write_access: bool) -> Result<windows::Win32::Foundation::HANDLE> {
    let path_wide = HSTRING::from(path.as_str());
    let access = if write_access {
        windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE.0
    } else {
        windows::Win32::Storage::FileSystem::FILE_GENERIC_READ.0
    };

    // Safety: path_wide is a valid null-terminated wide string;
    // flags are well-defined constants.
    let handle = CreateFileW(
        windows::core::PCWSTR(path_wide.as_ptr()),
        access,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        std::ptr::null(),
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        None,
    )
    .map_err(VfsWindowsError::CfApi)?;

    if handle == INVALID_HANDLE_VALUE {
        return Err(VfsWindowsError::PathNotFound(path.to_owned()));
    }

    Ok(handle)
}

// ── Public functions ──────────────────────────────────────────────────────────

/// Force-hydrate the placeholder at `path`.
///
/// Opens the file with write access, calls `CfHydratePlaceholder` over the
/// entire file range (`offset=0, length=-1` means full file), then closes
/// the handle.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the hydration fails.  In practice
/// this call will block until the registered hydration callback (Task 6) has
/// transferred all file data and called `CfExecute`.
pub fn hydrate(path: &Utf8Path) -> Result<()> {
    // Safety: open_for_cf returns a valid handle on Ok.
    let handle = unsafe { open_for_cf(path, true) }?;

    // Safety: handle is valid; length=-1 means "hydrate the whole file".
    let result = unsafe {
        CfHydratePlaceholder(handle, 0, -1, CF_HYDRATE_FLAG_NONE, std::ptr::null_mut())
    };

    // Safety: CloseHandle takes ownership of handle; called exactly once.
    unsafe { CloseHandle(handle) };

    result.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}

/// Dehydrate the file at `path` back to a placeholder.
///
/// Opens the file with write access, calls `CfDehydratePlaceholder` over the
/// full range, then closes the handle.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the dehydration fails.
pub fn dehydrate(path: &Utf8Path) -> Result<()> {
    // Safety: open_for_cf returns a valid handle on Ok.
    let handle = unsafe { open_for_cf(path, true) }?;

    // Safety: handle is valid; length=-1 means "dehydrate the whole file".
    let result = unsafe {
        CfDehydratePlaceholder(handle, 0, -1, CF_DEHYDRATE_FLAG_NONE, std::ptr::null_mut())
    };

    // Safety: CloseHandle takes ownership of handle; called exactly once.
    unsafe { CloseHandle(handle) };

    result.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}

/// Return `true` if the file at `path` has the `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`
/// attribute set, meaning it is a cloud placeholder that has not yet been
/// fully downloaded.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the file cannot be opened or
/// `GetFileInformationByHandle` fails.
pub fn is_virtual(path: &Utf8Path) -> Result<bool> {
    // Safety: open_for_cf returns a valid handle on Ok.
    let handle = unsafe { open_for_cf(path, false) }?;

    let mut info = BY_HANDLE_FILE_INFORMATION::default();

    // Safety: handle is valid; &mut info is a valid output pointer.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };

    // Safety: CloseHandle takes ownership of handle; called exactly once.
    unsafe { CloseHandle(handle) };

    ok.map_err(VfsWindowsError::CfApi)?;

    let recall_flag = FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0;
    Ok(info.dwFileAttributes & recall_flag != 0)
}

/// Return the [`VfsStatus`] of the file at `path` by inspecting its placeholder
/// state via `CfGetPlaceholderStateFromFileInfo`.
///
/// | CfAPI state                                          | VfsStatus         |
/// |------------------------------------------------------|-------------------|
/// | `CF_PLACEHOLDER_STATE_NO_STATES` (not a placeholder) | `Full`            |
/// | `CF_PLACEHOLDER_STATE_PARTIAL`                        | `Placeholder`     |
/// | `CF_PLACEHOLDER_STATE_IN_SYNC` + no RECALL attribute  | `Full`            |
/// | Any state with `RECALL_ON_DATA_ACCESS`                | `Placeholder`     |
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the file cannot be opened or queried.
pub fn status(path: &Utf8Path) -> Result<VfsStatus> {
    // Safety: open_for_cf returns a valid handle on Ok.
    let handle = unsafe { open_for_cf(path, false) }?;

    let mut info = BY_HANDLE_FILE_INFORMATION::default();

    // Safety: handle is valid; &mut info is a valid output pointer.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };

    if let Err(e) = ok {
        // Safety: close before returning error.
        unsafe { CloseHandle(handle) };
        return Err(VfsWindowsError::CfApi(e));
    }

    // Safety: &info is a valid pointer to a fully-initialised FILE_INFO struct.
    let placeholder_state = unsafe {
        CfGetPlaceholderStateFromFileInfo(
            &info as *const _ as *const _,
            windows::Win32::Storage::FileSystem::FileBasicInfo,
        )
    };

    // Safety: CloseHandle takes ownership of handle; called exactly once.
    unsafe { CloseHandle(handle) };

    let recall_flag = FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0;
    let has_recall = info.dwFileAttributes & recall_flag != 0;

    let vfs_status = if placeholder_state == CF_PLACEHOLDER_STATE_NO_STATES {
        VfsStatus::Full
    } else if placeholder_state == CF_PLACEHOLDER_STATE_PARTIAL || has_recall {
        VfsStatus::Placeholder
    } else if placeholder_state == CF_PLACEHOLDER_STATE_IN_SYNC && !has_recall {
        VfsStatus::Full
    } else {
        // Partially synced / in progress.
        VfsStatus::Syncing
    };

    Ok(vfs_status)
}
```

- [ ] Run (compilation check on Linux; ignored tests on Windows):

```bash
cargo build -p vfs-windows 2>&1
# Expected: compiles without errors

# Windows only:
cargo test -p vfs-windows --test hydration -- --ignored 2>&1
# Expected:
# test tests::placeholder_is_virtual ... ok
# test tests::status_of_placeholder_is_placeholder ... ok
# test tests::hydrate_dehydrate_do_not_error ... ok
```

- [ ] Commit:

```bash
git add crates/vfs-windows/src/hydration.rs
git commit -m "feat(vfs-windows): hydration.rs — hydrate, dehydrate, is_virtual, status"
```

---

## Task 5: pin.rs

- [ ] Write the failing test first. Create `crates/vfs-windows/tests/pin.rs`:

```rust
// tests/pin.rs
#[cfg(target_os = "windows")]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use std::time::SystemTime;
    use vfs_core::VfsFileItem;
    use vfs_windows::pin::set_pinned;
    use vfs_windows::placeholder::create_placeholder;
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    fn make_item(name: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size: 256,
            etag: "etag-pin".into(),
            file_id: "fid-pin".into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// Pin a placeholder, then unpin it — both operations must succeed.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn pin_then_unpin_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();
        create_placeholder(root, &make_item("pinme.txt")).unwrap();

        let file = root.join("pinme.txt");

        set_pinned(&file, true).expect("set_pinned(true) should succeed");
        set_pinned(&file, false).expect("set_pinned(false) should succeed");

        unregister_sync_root(root).unwrap();
    }

    /// Pinning a non-existent file must return an error, not panic.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn pin_nonexistent_returns_error() {
        let file = Utf8Path::new("C:\\nonexistent_vfs_test_file_xyz.txt");
        let result = set_pinned(file, true);
        assert!(result.is_err(), "pinning non-existent file should fail");
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}
```

- [ ] Run (expect failure — module missing):

```bash
cargo test -p vfs-windows --test pin 2>&1 | head -20
```

- [ ] Replace `crates/vfs-windows/src/pin.rs` with:

```rust
//! Pin-state management via CfSetPinState.

use camino::Utf8Path;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::CloudFilters::{
    CfSetPinState,
    CF_PIN_STATE_PINNED,
    CF_PIN_STATE_UNPINNED,
    CF_SET_PIN_FLAG_NONE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

use crate::error::{Result, VfsWindowsError};

/// Pin or unpin the file at `path`.
///
/// A pinned file is never automatically dehydrated by the OS; the user or
/// sync engine must explicitly call `dehydrate` to free disk space.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the file cannot be opened or the
/// pin-state change fails (e.g. file is not a CfAPI placeholder).
pub fn set_pinned(path: &Utf8Path, pinned: bool) -> Result<()> {
    let path_wide = HSTRING::from(path.as_str());

    // Safety: path_wide is a valid null-terminated wide string; flags are
    // well-defined Win32 constants that do not require additional constraints.
    let handle = unsafe {
        CreateFileW(
            windows::core::PCWSTR(path_wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(VfsWindowsError::CfApi)?;

    if handle == INVALID_HANDLE_VALUE {
        return Err(VfsWindowsError::PathNotFound(path.to_owned()));
    }

    let pin_state = if pinned {
        CF_PIN_STATE_PINNED
    } else {
        CF_PIN_STATE_UNPINNED
    };

    // Safety: handle is valid (checked above); pin_state is a valid CfAPI
    // constant.  CloseHandle is always called before returning.
    let result = unsafe { CfSetPinState(handle, pin_state, CF_SET_PIN_FLAG_NONE, std::ptr::null_mut()) };

    // Safety: CloseHandle takes ownership of handle; called exactly once.
    unsafe { CloseHandle(handle) };

    result.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}
```

- [ ] Run (compilation check + Windows integration):

```bash
cargo build -p vfs-windows 2>&1
# Expected: compiles without errors

# Windows only:
cargo test -p vfs-windows --test pin -- --ignored 2>&1
# Expected:
# test tests::pin_then_unpin_succeeds ... ok
# test tests::pin_nonexistent_returns_error ... ok
```

- [ ] Commit:

```bash
git add crates/vfs-windows/src/pin.rs
git commit -m "feat(vfs-windows): pin.rs — CfSetPinState with CF_PIN_STATE_PINNED/UNPINNED"
```

---

## Task 6: callback.rs

- [ ] Write the failing test first. Create `crates/vfs-windows/tests/callback.rs`:

```rust
// tests/callback.rs
#[cfg(target_os = "windows")]
mod tests {
    use std::sync::Arc;
    use camino::Utf8Path;
    use tokio::sync::mpsc;
    use vfs_windows::callback::{
        HydrationCallbackContext, HydrationRequest, RawCallbackInfo,
        register_hydration_callback, unregister_hydration_callback,
    };
    use vfs_windows::registration::{register_sync_root, unregister_sync_root};

    /// Registers a hydration callback and immediately unregisters it.
    /// Verifies the CF_CONNECTION_KEY is non-zero.
    #[test]
    #[ignore = "requires Windows + NTFS volume"]
    fn register_and_unregister_callback() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap();

        register_sync_root(root, "TestProvider", "1.0.0").unwrap();

        let (tx, _rx) = mpsc::channel::<HydrationRequest>(8);
        let ctx = Arc::new(HydrationCallbackContext { tx });

        let key = register_hydration_callback(root, ctx)
            .expect("register_hydration_callback should succeed");

        // CF_CONNECTION_KEY is a non-zero value when registration succeeded.
        assert_ne!(key.0, 0, "connection key must be non-zero");

        unregister_hydration_callback(key).expect("unregister should succeed");
        unregister_sync_root(root).unwrap();
    }

    /// Verifies the HydrationRequest struct has the expected fields.
    /// This is a compile-time / pure-logic test.
    #[test]
    fn hydration_request_fields_accessible() {
        use camino::Utf8PathBuf;
        let req = HydrationRequest {
            path: Utf8PathBuf::from("C:\\sync\\file.txt"),
            offset: 0,
            length: 4096,
            callback_info: RawCallbackInfo {
                connection_key: windows::Win32::Storage::CloudFilters::CF_CONNECTION_KEY(1),
                transfer_key: 42,
                request_key: 99,
            },
        };
        assert_eq!(req.offset, 0);
        assert_eq!(req.length, 4096);
        assert_eq!(req.path.as_str(), "C:\\sync\\file.txt");
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}
```

- [ ] Run (expect failure — module missing):

```bash
cargo test -p vfs-windows --test callback 2>&1 | head -20
```

- [ ] Replace `crates/vfs-windows/src/callback.rs` with:

```rust
//! Hydration callback registration and dispatch.
//!
//! Windows calls into our process via the `CF_CALLBACK_TYPE_FETCH_DATA`
//! callback whenever a user or application opens a virtual file.  We forward
//! the request to the sync engine over a tokio mpsc channel and expect the
//! caller to respond with a `CfExecute(CF_OPERATION_TYPE_TRANSFER_DATA)` call.

use std::sync::Arc;

use camino::Utf8PathBuf;
use tokio::sync::mpsc;

use windows::Win32::Storage::CloudFilters::{
    CfConnectSyncRoot,
    CfDisconnectSyncRoot,
    CF_CALLBACK,
    CF_CALLBACK_INFO,
    CF_CALLBACK_PARAMETERS,
    CF_CALLBACK_REGISTRATION,
    CF_CALLBACK_TYPE_FETCH_DATA,
    CF_CALLBACK_TYPE_NONE,
    CF_CONNECTION_KEY,
    CF_CONNECT_FLAG_NONE,
};
use windows::core::HSTRING;

use crate::error::{Result, VfsWindowsError};

// ── Public types ──────────────────────────────────────────────────────────────

/// Opaque callback info needed to call `CfExecute` from the sync engine.
///
/// The sync engine receives a [`HydrationRequest`] on the channel, downloads
/// the file content, then calls `CfExecute` using these fields.
#[derive(Debug, Clone)]
pub struct RawCallbackInfo {
    /// The connection key returned by `CfConnectSyncRoot`.
    pub connection_key: CF_CONNECTION_KEY,
    /// Identifies the specific transfer operation (from `CF_CALLBACK_INFO`).
    pub transfer_key: i64,
    /// Identifies the request within the transfer.
    pub request_key: i64,
}

/// A request sent to the sync engine asking it to provide file data.
#[derive(Debug)]
pub struct HydrationRequest {
    /// Absolute path of the file being hydrated.
    pub path: Utf8PathBuf,
    /// Byte offset of the range the OS is requesting.
    pub offset: u64,
    /// Number of bytes requested.
    pub length: u64,
    /// Opaque info the sync engine needs to call `CfExecute`.
    pub callback_info: RawCallbackInfo,
}

/// Context shared between the callback registration and the callback function.
///
/// Wrapped in `Arc` so it can be placed in the `CallbackContext` pointer passed
/// to `CfConnectSyncRoot` and recovered in the static callback.
pub struct HydrationCallbackContext {
    /// Channel used to forward hydration requests to the async sync engine.
    pub tx: mpsc::Sender<HydrationRequest>,
}

// Internal wrapper kept alive for the lifetime of the connection.
struct CallbackState {
    ctx: Arc<HydrationCallbackContext>,
}

// ── Static callback ───────────────────────────────────────────────────────────

/// The `extern "system"` hydration callback invoked by Windows.
///
/// # Safety
///
/// This function is called by the Windows kernel via a registered callback.
/// The `callback_info` pointer is guaranteed valid for the duration of the
/// callback invocation.  We extract the data we need and send it on the
/// channel without retaining any pointers after the function returns.
unsafe extern "system" fn fetch_data_callback(
    callback_info: *const CF_CALLBACK_INFO,
    callback_params: *const CF_CALLBACK_PARAMETERS,
) {
    // Safety: Windows guarantees callback_info is a valid pointer here.
    let info = &*callback_info;
    // Safety: same guarantee for callback_params.
    let params = &*callback_params;

    // Recover the Arc<HydrationCallbackContext> from the CallbackContext field.
    // Safety: we stored a raw pointer to a Box<CallbackState> in CfConnectSyncRoot;
    // Windows keeps it alive until CfDisconnectSyncRoot is called.
    let state = &*(info.CallbackContext as *const CallbackState);

    // Extract the file path from the volume GUID + file name stored in the info.
    let path = {
        // CF_CALLBACK_INFO.NormalizedPath is a null-terminated wide string.
        let ptr = info.NormalizedPath.0;
        if ptr.is_null() {
            return;
        }
        let wide: &[u16] = {
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            std::slice::from_raw_parts(ptr, len)
        };
        Utf8PathBuf::from(String::from_utf16_lossy(wide))
    };

    // Extract offset and length from the FETCH_DATA parameters.
    // Safety: we only register CF_CALLBACK_TYPE_FETCH_DATA so this union arm
    // is always valid when this callback fires.
    let fetch = params.Anonymous.FetchData;
    let offset = fetch.RequiredFileOffset as u64;
    let length = fetch.RequiredLength as u64;

    let req = HydrationRequest {
        path,
        offset,
        length,
        callback_info: RawCallbackInfo {
            connection_key: info.ConnectionKey,
            transfer_key: info.TransferKey,
            request_key: info.RequestKey,
        },
    };

    // Non-blocking send — if the channel is full we drop the request.
    // The OS will re-trigger the callback when the app retries the open.
    let _ = state.ctx.tx.try_send(req);
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Register a `CF_CALLBACK_TYPE_FETCH_DATA` callback for the sync root at
/// `root`.
///
/// Returns a [`CF_CONNECTION_KEY`] that must be passed to
/// [`unregister_hydration_callback`] when the sync root is torn down.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if `CfConnectSyncRoot` fails.
pub fn register_hydration_callback(
    root: &camino::Utf8Path,
    ctx: Arc<HydrationCallbackContext>,
) -> Result<CF_CONNECTION_KEY> {
    // Leak a Box<CallbackState> so its address stays stable for the lifetime
    // of the connection.  It is reclaimed in unregister_hydration_callback.
    let state = Box::new(CallbackState { ctx });
    let state_ptr = Box::into_raw(state) as *mut _;

    let callbacks = [
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_FETCH_DATA,
            Callback: CF_CALLBACK {
                FetchData: Some(fetch_data_callback),
            },
        },
        // Sentinel entry required by CfConnectSyncRoot.
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: CF_CALLBACK {
                FetchData: None,
            },
        },
    ];

    let root_wide = HSTRING::from(root.as_str());

    let mut connection_key = CF_CONNECTION_KEY(0);

    // Safety: callbacks array ends with CF_CALLBACK_TYPE_NONE sentinel as
    // required by CfConnectSyncRoot; state_ptr is valid for the duration of
    // the connection; root_wide is a valid null-terminated wide string.
    unsafe {
        CfConnectSyncRoot(
            windows::core::PCWSTR(root_wide.as_ptr()),
            callbacks.as_ptr(),
            state_ptr,
            CF_CONNECT_FLAG_NONE,
            &mut connection_key,
        )
    }
    .map_err(|e| {
        // Safety: if CfConnectSyncRoot failed, the callback was never
        // installed, so we must reclaim the leaked Box here.
        unsafe { drop(Box::from_raw(state_ptr as *mut CallbackState)) };
        VfsWindowsError::CfApi(e)
    })?;

    Ok(connection_key)
}

/// Unregister a previously registered hydration callback.
///
/// Calls `CfDisconnectSyncRoot` to stop Windows from delivering callbacks, then
/// reclaims the `CallbackState` that was leaked in `register_hydration_callback`.
///
/// # Safety
///
/// The `key` must be the exact value returned by a successful call to
/// `register_hydration_callback` and must not be used again after this call.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if `CfDisconnectSyncRoot` fails.
pub fn unregister_hydration_callback(key: CF_CONNECTION_KEY) -> Result<()> {
    // Safety: key is a valid connection key obtained from CfConnectSyncRoot.
    unsafe { CfDisconnectSyncRoot(key) }.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}
```

- [ ] Run (compilation check + Windows integration):

```bash
cargo build -p vfs-windows 2>&1
# Expected: compiles without errors

cargo test -p vfs-windows --test callback 2>&1
# On Linux: hydration_request_fields_accessible test is not cfg-gated, but
#           it references windows types — verify it compiles.
# On Windows: 1 non-ignored test + 1 ignored test.
# Run ignored tests on Windows:
# cargo test -p vfs-windows --test callback -- --ignored
# Expected:
# test tests::register_and_unregister_callback ... ok
```

- [ ] Commit:

```bash
git add crates/vfs-windows/src/callback.rs
git commit -m "feat(vfs-windows): callback.rs — CF_CALLBACK_TYPE_FETCH_DATA registration + dispatch"
```

---

## Task 7: VfsWindows struct + Vfs impl

- [ ] Write the failing test first. Create `crates/vfs-windows/tests/vfs_impl.rs`:

```rust
// tests/vfs_impl.rs
#[cfg(target_os = "windows")]
mod tests {
    use std::sync::Arc;
    use camino::{Utf8Path, Utf8PathBuf};
    use std::time::SystemTime;
    use tokio::sync::mpsc;
    use vfs_core::{Vfs, VfsFileItem, VfsStatus};
    use vfs_windows::{VfsWindows, HydrationRequest};

    fn make_item(name: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size: 512,
            etag: "etag-vfs".into(),
            file_id: "fid-vfs".into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// Full lifecycle: VfsWindows::new → create_placeholder → is_virtual →
    /// set_pinned → dehydrate → drop (auto-unregisters).
    #[tokio::test]
    #[ignore = "requires Windows + NTFS volume"]
    async fn full_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap().to_owned();

        let (tx, _rx) = mpsc::channel::<HydrationRequest>(16);
        let vfs = VfsWindows::new(root.clone(), "TestProvider", tx)
            .expect("VfsWindows::new should succeed");

        let item = make_item("test.txt");
        vfs.create_placeholder(&item).await.expect("create_placeholder");

        let file = root.join("test.txt");
        let virtual_flag = vfs.is_virtual(&file).await.unwrap();
        assert!(virtual_flag, "newly created placeholder must be virtual");

        let s = vfs.status(&file).await.unwrap();
        assert_eq!(s, VfsStatus::Placeholder);

        vfs.set_pinned(&file, true).await.expect("set_pinned(true)");
        vfs.set_pinned(&file, false).await.expect("set_pinned(false)");
        vfs.dehydrate(&file).await.expect("dehydrate");

        // Drop calls unregister_hydration_callback + unregister_sync_root.
        drop(vfs);
    }

    /// VfsWindows must be usable as a trait object.
    #[test]
    fn vfs_windows_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VfsWindows>();
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}
```

- [ ] Run (expect failure — module missing):

```bash
cargo test -p vfs-windows --test vfs_impl 2>&1 | head -20
```

- [ ] Replace `crates/vfs-windows/src/vfs_impl.rs` with:

```rust
//! `VfsWindows` — the public struct implementing `vfs_core::Vfs` on Windows.

use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use tokio::sync::mpsc;

use vfs_core::{Vfs, VfsError, VfsFileItem, VfsStatus};

use crate::callback::{
    register_hydration_callback, unregister_hydration_callback,
    HydrationCallbackContext, HydrationRequest,
};
use crate::error::VfsWindowsError;
use crate::hydration;
use crate::pin;
use crate::placeholder;
use crate::registration::{register_sync_root, unregister_sync_root};

use windows::Win32::Storage::CloudFilters::CF_CONNECTION_KEY;

/// A VFS implementation backed by the Windows CloudFiles API.
///
/// On construction (`new`) this registers a CfAPI sync root for the given
/// `root` directory and installs a `CF_CALLBACK_TYPE_FETCH_DATA` callback.
/// On drop it removes the callback and unregisters the sync root.
pub struct VfsWindows {
    root: Utf8PathBuf,
    callback_key: CF_CONNECTION_KEY,
    hydration_tx: mpsc::Sender<HydrationRequest>,
}

impl VfsWindows {
    /// Create a new `VfsWindows` for `root`.
    ///
    /// Calls `CfRegisterSyncRoot` then `CfConnectSyncRoot`.  Any hydration
    /// requests from Windows will be forwarded to `hydration_tx`.
    ///
    /// # Errors
    ///
    /// Returns [`VfsError`] if either CfAPI call fails.
    pub fn new(
        root: Utf8PathBuf,
        provider_name: &str,
        hydration_tx: mpsc::Sender<HydrationRequest>,
    ) -> Result<Self, VfsError> {
        register_sync_root(&root, provider_name, "1.0.0")
            .map_err(VfsError::from)?;

        let ctx = Arc::new(HydrationCallbackContext {
            tx: hydration_tx.clone(),
        });

        let callback_key = register_hydration_callback(&root, ctx)
            .map_err(VfsError::from)?;

        Ok(Self {
            root,
            callback_key,
            hydration_tx,
        })
    }
}

impl Drop for VfsWindows {
    /// Unregister the hydration callback and the sync root on drop.
    ///
    /// Errors are logged but not propagated because `Drop` cannot return a
    /// `Result`.
    fn drop(&mut self) {
        if let Err(e) = unregister_hydration_callback(self.callback_key) {
            eprintln!("vfs-windows: failed to unregister hydration callback: {e}");
        }
        if let Err(e) = unregister_sync_root(&self.root) {
            eprintln!("vfs-windows: failed to unregister sync root: {e}");
        }
    }
}

// ── Vfs trait implementation ──────────────────────────────────────────────────

#[async_trait::async_trait]
impl Vfs for VfsWindows {
    /// Create a dehydrated placeholder at the path described in `item`.
    ///
    /// The path in `item` is treated as relative to the sync root.
    async fn create_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError> {
        let root = self.root.clone();
        let item = item.clone();
        tokio::task::spawn_blocking(move || {
            placeholder::create_placeholder(&root, &item).map_err(VfsError::from)
        })
        .await
        .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    /// Update the metadata of an existing placeholder at `path`.
    async fn update_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError> {
        let full_path = self.root.join(&item.path);
        let item = item.clone();
        tokio::task::spawn_blocking(move || {
            placeholder::update_placeholder(&full_path, &item).map_err(VfsError::from)
        })
        .await
        .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    /// Force-hydrate the placeholder at `path`.
    async fn hydrate(&self, path: &Utf8Path) -> Result<(), VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            hydration::hydrate(&path).map_err(VfsError::from)
        })
        .await
        .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    /// Dehydrate the file at `path` back to a placeholder.
    async fn dehydrate(&self, path: &Utf8Path) -> Result<(), VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            hydration::dehydrate(&path).map_err(VfsError::from)
        })
        .await
        .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    /// Return `true` if the file at `path` is a cloud placeholder.
    async fn is_virtual(&self, path: &Utf8Path) -> Result<bool, VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            hydration::is_virtual(&path).map_err(VfsError::from)
        })
        .await
        .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    /// Return the current [`VfsStatus`] of the file at `path`.
    async fn status(&self, path: &Utf8Path) -> Result<VfsStatus, VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            hydration::status(&path).map_err(VfsError::from)
        })
        .await
        .map_err(|e| VfsError::Backend(e.to_string()))?
    }

    /// Pin or unpin the file at `path`.
    async fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<(), VfsError> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            pin::set_pinned(&path, pinned).map_err(VfsError::from)
        })
        .await
        .map_err(|e| VfsError::Backend(e.to_string()))?
    }
}
```

- [ ] Update `vfs-core`'s `Vfs` trait to match the signature used by `VfsWindows` (which takes `&VfsFileItem` rather than `path + item`). Update `crates/vfs-core/src/lib.rs`:

```rust
#[async_trait]
pub trait Vfs: Send + Sync {
    /// Create a dehydrated placeholder for `item`.
    /// The `item.path` is relative to the sync root.
    async fn create_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError>;

    /// Update metadata of an existing placeholder.
    async fn update_placeholder(&self, item: &VfsFileItem) -> Result<(), VfsError>;

    /// Force-hydrate the file at `path`.
    async fn hydrate(&self, path: &Utf8Path) -> Result<(), VfsError>;

    /// Dehydrate the file at `path` back to a placeholder.
    async fn dehydrate(&self, path: &Utf8Path) -> Result<(), VfsError>;

    /// Return `true` if the file at `path` is a virtual placeholder.
    async fn is_virtual(&self, path: &Utf8Path) -> Result<bool, VfsError>;

    /// Return the current [`VfsStatus`] of `path`.
    async fn status(&self, path: &Utf8Path) -> Result<VfsStatus, VfsError>;

    /// Pin or unpin `path`.
    async fn set_pinned(&self, path: &Utf8Path, pinned: bool) -> Result<(), VfsError>;
}
```

Update `vfs-off`'s `VfsOff` implementation to match the new signatures:

```rust
// In crates/vfs-off/src/lib.rs — update the impl:
#[async_trait]
impl Vfs for VfsOff {
    async fn create_placeholder(&self, _item: &VfsFileItem) -> Result<(), VfsError> {
        Ok(())
    }
    async fn update_placeholder(&self, _item: &VfsFileItem) -> Result<(), VfsError> {
        Ok(())
    }
    async fn hydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> { Ok(()) }
    async fn dehydrate(&self, _path: &Utf8Path) -> Result<(), VfsError> { Ok(()) }
    async fn is_virtual(&self, _path: &Utf8Path) -> Result<bool, VfsError> { Ok(false) }
    async fn status(&self, _path: &Utf8Path) -> Result<VfsStatus, VfsError> {
        Ok(VfsStatus::Full)
    }
    async fn set_pinned(&self, _path: &Utf8Path, _pinned: bool) -> Result<(), VfsError> {
        Ok(())
    }
}
```

Add `async-trait` to `vfs-windows/Cargo.toml`:

```toml
[dependencies]
async-trait = { workspace = true }
```

- [ ] Run (compilation check + cross-platform test):

```bash
cargo build --workspace 2>&1
# Expected: compiles without errors on Linux
# vfs_windows_is_send_sync is a compile-time assertion — verify:
cargo test -p vfs-windows 2>&1
# On Linux: 0 tests collected (all Windows-gated)

# Windows only:
cargo test -p vfs-windows --test vfs_impl -- --ignored 2>&1
# Expected:
# test tests::full_lifecycle ... ok
```

- [ ] Commit:

```bash
git add crates/vfs-windows/src/vfs_impl.rs \
        crates/vfs-windows/Cargo.toml \
        crates/vfs-core/src/lib.rs \
        crates/vfs-off/src/lib.rs
git commit -m "feat(vfs-windows): VfsWindows struct + Vfs trait impl with spawn_blocking dispatch"
```

---

## Task 8: Integration test skeleton

- [ ] Create `crates/vfs-windows/tests/integration.rs`:

```rust
//! Integration test skeleton for vfs-windows.
//!
//! All tests are marked `#[ignore]` because they require:
//!   - A real Windows machine
//!   - An NTFS-formatted volume (not FAT32 / exFAT)
//!   - Developer Mode enabled OR administrator privileges
//!
//! # How to run
//!
//! ```bash
//! cargo test --target x86_64-pc-windows-msvc -p vfs-windows --test integration -- --ignored
//! ```
//!
//! # CI
//!
//! These tests should be added to a dedicated Windows runner job in the CI
//! pipeline and run only on pushes that touch `crates/vfs-windows/`.

#[cfg(target_os = "windows")]
mod tests {
    use std::time::SystemTime;
    use camino::{Utf8Path, Utf8PathBuf};
    use tokio::sync::mpsc;
    use vfs_core::{Vfs, VfsFileItem, VfsStatus};
    use vfs_windows::{VfsWindows, HydrationRequest};

    fn make_item(name: &str, size: u64, file_id: &str) -> VfsFileItem {
        VfsFileItem {
            path: Utf8PathBuf::from(name),
            size,
            etag: "integration-etag".into(),
            file_id: file_id.into(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// Creates a VfsWindows instance for a fresh temp directory, creates a
    /// 1 KB placeholder, checks is_virtual returns true, checks status is
    /// Placeholder, attempts dehydrate (no-op on already-dehydrated file),
    /// and verifies VfsWindows drops without panicking.
    ///
    /// Note: hydrate() is NOT called here because the actual data transfer
    /// requires a running sync engine with a registered FETCH_DATA handler.
    #[tokio::test]
    #[ignore = "requires Windows + NTFS volume"]
    async fn create_placeholder_is_virtual_status() {
        // Arrange: create a temp NTFS directory.
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let root = Utf8Path::from_path(dir.path())
            .expect("temp dir path is not valid UTF-8")
            .to_owned();

        // Arrange: create a channel for hydration requests.
        let (tx, mut rx) = mpsc::channel::<HydrationRequest>(16);

        // Act: construct VfsWindows (registers sync root + callback).
        let vfs = VfsWindows::new(root.clone(), "IntegrationTestProvider", tx)
            .expect("VfsWindows::new should succeed on NTFS volume");

        // Act: create a 1 KB placeholder.
        let item = make_item("integration_test.txt", 1024, "integration-file-id-001");
        vfs.create_placeholder(&item)
            .await
            .expect("create_placeholder should succeed");

        let file_path = root.join("integration_test.txt");
        assert!(file_path.exists(), "placeholder file must exist on disk");

        // Assert: file is virtual (has RECALL_ON_DATA_ACCESS attribute).
        let virtual_flag = vfs
            .is_virtual(&file_path)
            .await
            .expect("is_virtual should succeed");
        assert!(virtual_flag, "newly created placeholder must be virtual");

        // Assert: status is Placeholder.
        let s = vfs
            .status(&file_path)
            .await
            .expect("status should succeed");
        assert_eq!(
            s,
            VfsStatus::Placeholder,
            "status of a fresh placeholder must be Placeholder"
        );

        // Act: dehydrate (no-op on already-dehydrated placeholder — must not error).
        vfs.dehydrate(&file_path)
            .await
            .expect("dehydrate should succeed on placeholder");

        // Assert: no hydration requests were received (no open/read occurred).
        assert!(
            rx.try_recv().is_err(),
            "no hydration request should have been sent during this test"
        );

        // Act: drop VfsWindows — verifies unregister does not panic.
        drop(vfs);
    }

    /// Verify update_placeholder replaces the metadata of an existing placeholder.
    #[tokio::test]
    #[ignore = "requires Windows + NTFS volume"]
    async fn update_placeholder_changes_size() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let (tx, _rx) = mpsc::channel::<HydrationRequest>(8);

        let vfs = VfsWindows::new(root.clone(), "IntegrationTestProvider", tx).unwrap();

        let original = make_item("update_test.txt", 512, "upd-fid");
        vfs.create_placeholder(&original).await.unwrap();

        let updated = VfsFileItem {
            path: Utf8PathBuf::from("update_test.txt"),
            size: 8192,
            etag: "new-etag".into(),
            file_id: "upd-fid".into(),
            last_modified: SystemTime::UNIX_EPOCH,
        };
        let file_path = root.join("update_test.txt");
        vfs.update_placeholder(&updated)
            .await
            .expect("update_placeholder should succeed");

        // File should still be virtual after metadata update.
        assert!(vfs.is_virtual(&file_path).await.unwrap());
        drop(vfs);
    }

    /// Verify set_pinned does not error on a freshly created placeholder.
    #[tokio::test]
    #[ignore = "requires Windows + NTFS volume"]
    async fn set_pinned_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(dir.path()).unwrap().to_owned();
        let (tx, _rx) = mpsc::channel::<HydrationRequest>(8);

        let vfs = VfsWindows::new(root.clone(), "IntegrationTestProvider", tx).unwrap();

        let item = make_item("pin_test.txt", 256, "pin-fid");
        vfs.create_placeholder(&item).await.unwrap();

        let file = root.join("pin_test.txt");
        vfs.set_pinned(&file, true).await.expect("pin should succeed");
        vfs.set_pinned(&file, false).await.expect("unpin should succeed");

        drop(vfs);
    }
}

/// On non-Windows platforms this module is empty; it must still compile.
#[cfg(not(target_os = "windows"))]
fn placeholder_compilation_guard() {}
```

- [ ] Verify the file compiles on all platforms:

```bash
cargo build -p vfs-windows 2>&1
# Expected: no errors
```

- [ ] Run the full vfs-windows test suite (non-ignored tests only — should pass on Linux):

```bash
cargo test -p vfs-windows 2>&1
# Expected: all non-ignored tests pass
```

- [ ] Run the complete workspace test suite to check for regressions:

```bash
cargo test --workspace 2>&1
# Expected: all tests pass (no regressions in vfs-core, vfs-off, sync-engine)
```

- [ ] Run all ignored integration tests on Windows:

```bash
# Windows only:
cargo test --target x86_64-pc-windows-msvc -p vfs-windows --test integration -- --ignored 2>&1
# Expected:
# test tests::create_placeholder_is_virtual_status ... ok
# test tests::update_placeholder_changes_size ... ok
# test tests::set_pinned_roundtrip ... ok
```

- [ ] Commit:

```bash
git add crates/vfs-windows/tests/integration.rs
git commit -m "feat(vfs-windows): add integration test skeleton (ignored, requires Windows NTFS)"
```

---

## Completion checklist

- [ ] `cargo build -p vfs-windows` — compiles on Linux (cfg gates suppress Windows-only code)
- [ ] `cargo test -p vfs-windows` — all non-ignored tests pass on Linux
- [ ] `cargo test --workspace` — no regressions across vfs-core, vfs-off, sync-engine
- [ ] On Windows: `cargo test -p vfs-windows -- --ignored` — all 12 ignored integration tests pass
- [ ] All 8 tasks committed individually with descriptive messages
- [ ] Every `unsafe` block has a one-line safety comment
- [ ] No `unwrap()` in library code paths outside tests
- [ ] No `todo!()` / `unimplemented!()` in committed library code
- [ ] `VfsWindows` implements `Vfs`, is `Send + Sync`, and can be used as `Box<dyn Vfs>`

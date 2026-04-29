//! Placeholder creation and update via CfCreatePlaceholders / CfUpdatePlaceholder.

use camino::Utf8Path;
use std::time::SystemTime;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::CloudFilters::{
    CfCreatePlaceholders, CfUpdatePlaceholder, CF_CREATE_FLAG_NONE, CF_FS_METADATA,
    CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC, CF_PLACEHOLDER_CREATE_INFO,
    CF_UPDATE_FLAG_MARK_IN_SYNC,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_WRITE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};

use vfs_core::VfsFileItem;

use crate::error::{Result, VfsWindowsError};

/// Convert a [`SystemTime`] to a Win32 [`FILETIME`].
///
/// FILETIME is 100-nanosecond intervals since 1601-01-01 00:00:00 UTC.
fn system_time_to_filetime(t: SystemTime) -> FILETIME {
    // Duration from 1601-01-01 to 1970-01-01 in 100-ns intervals.
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;
    let since_unix = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
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
    let identity = item.file_id.as_bytes();

    let filename = item.path.file_name().ok_or_else(|| {
        VfsWindowsError::StringConversion(format!("path has no filename: {}", item.path))
    })?;
    let filename_wide = HSTRING::from(filename);

    let last_write = system_time_to_filetime(item.last_modified);
    let last_write_large: i64 = unsafe {
        std::mem::transmute(
            ((last_write.dwHighDateTime as u64) << 32) | (last_write.dwLowDateTime as u64),
        )
    };

    let mut create_info = CF_PLACEHOLDER_CREATE_INFO {
        RelativeFileName: windows::core::PCWSTR(filename_wide.as_ptr()),
        FsMetadata: CF_FS_METADATA {
            FileSize: item.size as i64,
            BasicInfo: FILE_BASIC_INFO {
                LastWriteTime: last_write_large,
                ChangeTime: last_write_large,
                CreationTime: last_write_large,
                LastAccessTime: last_write_large,
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
    let last_write_large: i64 = unsafe {
        std::mem::transmute(
            ((last_write.dwHighDateTime as u64) << 32) | (last_write.dwLowDateTime as u64),
        )
    };

    // Safety: CreateFileW is an FFI call; path_wide is a valid null-terminated
    // wide string. The returned HANDLE is checked immediately.
    let handle = unsafe {
        CreateFileW(
            windows::core::PCWSTR(path_wide.as_ptr()),
            FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(VfsWindowsError::CfApi)?;

    if handle == INVALID_HANDLE_VALUE {
        return Err(VfsWindowsError::PathNotFound(path.to_owned()));
    }

    let fs_metadata = CF_FS_METADATA {
        FileSize: item.size as i64,
        BasicInfo: FILE_BASIC_INFO {
            LastWriteTime: last_write_large,
            ChangeTime: last_write_large,
            CreationTime: last_write_large,
            LastAccessTime: last_write_large,
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
    unsafe { CloseHandle(handle).ok() };

    result.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}

//! Pin-state management via CfSetPinState.

use camino::Utf8Path;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::CloudFilters::{
    CfSetPinState, CF_PIN_STATE_PINNED, CF_PIN_STATE_UNPINNED, CF_SET_PIN_FLAG_NONE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

use crate::error::{Result, VfsWindowsError};

/// Pin or unpin the file at `path`.
///
/// A pinned file is never automatically dehydrated by the OS.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the file cannot be opened or the
/// pin-state change fails.
pub fn set_pinned(path: &Utf8Path, pinned: bool) -> Result<()> {
    let path_wide = HSTRING::from(path.as_str());

    // Safety: path_wide is a valid null-terminated wide string.
    let handle = unsafe {
        CreateFileW(
            windows::core::PCWSTR(path_wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
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

    // Safety: handle is valid; CloseHandle is always called before returning.
    let result = unsafe {
        CfSetPinState(handle, pin_state, CF_SET_PIN_FLAG_NONE, std::ptr::null_mut())
    };

    // Safety: CloseHandle takes ownership of handle; called exactly once.
    unsafe { CloseHandle(handle).ok() };

    result.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}

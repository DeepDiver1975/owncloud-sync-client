//! Hydration, dehydration, virtual-status query, and VfsStatus mapping.

use camino::Utf8Path;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::CloudFilters::{
    CfDehydratePlaceholder, CfGetPlaceholderStateFromFileInfo, CfHydratePlaceholder,
    CF_DEHYDRATE_FLAG_NONE, CF_HYDRATE_FLAG_NONE, CF_PLACEHOLDER_STATE_IN_SYNC,
    CF_PLACEHOLDER_STATE_NO_STATES, CF_PLACEHOLDER_STATE_PARTIAL,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::WindowsProgramming::FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;

use vfs_core::VfsStatus;

use crate::error::{Result, VfsWindowsError};

/// Open a file handle suitable for CfAPI operations.
///
/// # Safety
///
/// The caller is responsible for closing the returned handle with `CloseHandle`.
unsafe fn open_for_cf(
    path: &Utf8Path,
    write_access: bool,
) -> Result<windows::Win32::Foundation::HANDLE> {
    let path_wide = HSTRING::from(path.as_str());
    let access = if write_access {
        windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE.0
    } else {
        windows::Win32::Storage::FileSystem::FILE_GENERIC_READ.0
    };

    // Safety: path_wide is a valid null-terminated wide string.
    let handle = CreateFileW(
        windows::core::PCWSTR(path_wide.as_ptr()),
        access,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        None,
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

/// Force-hydrate the placeholder at `path`.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the hydration fails.
pub fn hydrate(path: &Utf8Path) -> Result<()> {
    // Safety: open_for_cf returns a valid handle on Ok.
    let handle = unsafe { open_for_cf(path, true) }?;

    // Safety: handle is valid; length=-1 means "hydrate the whole file".
    let result = unsafe {
        CfHydratePlaceholder(handle, 0, -1, CF_HYDRATE_FLAG_NONE, std::ptr::null_mut())
    };

    // Safety: CloseHandle takes ownership of handle; called exactly once.
    unsafe { CloseHandle(handle).ok() };

    result.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}

/// Dehydrate the file at `path` back to a placeholder.
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
    unsafe { CloseHandle(handle).ok() };

    result.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}

/// Return `true` if the file at `path` has the `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`
/// attribute set, meaning it is a cloud placeholder not yet fully downloaded.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if the file cannot be opened or queried.
pub fn is_virtual(path: &Utf8Path) -> Result<bool> {
    // Safety: open_for_cf returns a valid handle on Ok.
    let handle = unsafe { open_for_cf(path, false) }?;

    let mut info = BY_HANDLE_FILE_INFORMATION::default();

    // Safety: handle is valid; &mut info is a valid output pointer.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };

    // Safety: CloseHandle takes ownership of handle; called exactly once.
    unsafe { CloseHandle(handle).ok() };

    ok.map_err(VfsWindowsError::CfApi)?;

    let recall_flag = FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0;
    Ok(info.dwFileAttributes & recall_flag != 0)
}

/// Return the [`VfsStatus`] of the file at `path`.
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
        unsafe { CloseHandle(handle).ok() };
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
    unsafe { CloseHandle(handle).ok() };

    let recall_flag = FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0;
    let has_recall = info.dwFileAttributes & recall_flag != 0;

    let vfs_status = if placeholder_state == CF_PLACEHOLDER_STATE_NO_STATES {
        VfsStatus::Full
    } else if placeholder_state == CF_PLACEHOLDER_STATE_PARTIAL || has_recall {
        VfsStatus::Placeholder
    } else if placeholder_state == CF_PLACEHOLDER_STATE_IN_SYNC && !has_recall {
        VfsStatus::Full
    } else {
        VfsStatus::Syncing
    };

    Ok(vfs_status)
}

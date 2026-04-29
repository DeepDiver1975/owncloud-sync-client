//! Sync root registration and unregistration via CfRegisterSyncRoot /
//! CfUnregisterSyncRoot.

use camino::Utf8Path;

use crate::error::{Result, VfsWindowsError};

use windows::core::HSTRING;
use windows::Win32::Storage::CloudFilters::{
    CfRegisterSyncRoot, CfUnregisterSyncRoot, CF_HYDRATION_POLICY, CF_HYDRATION_POLICY_MODIFIER,
    CF_HYDRATION_POLICY_PARTIAL, CF_POPULATION_POLICY, CF_POPULATION_POLICY_MODIFIER,
    CF_POPULATION_POLICY_PARTIAL, CF_REGISTER_FLAG_NONE, CF_SYNC_REGISTRATION,
};

/// Register `path` as a CfAPI sync root.
pub fn register_sync_root(
    path: &Utf8Path,
    provider_name: &str,
    provider_version: &str,
) -> Result<()> {
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

    // Safety: all pointer fields in registration reference data that lives at
    // least as long as this stack frame.
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
pub fn unregister_sync_root(path: &Utf8Path) -> Result<()> {
    let path_wide = HSTRING::from(path.as_str());
    // Safety: path_wide is a valid wide string.
    unsafe { CfUnregisterSyncRoot(&path_wide) }.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}

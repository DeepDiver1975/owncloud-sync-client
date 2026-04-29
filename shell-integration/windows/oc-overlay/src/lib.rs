//! oc-overlay: IShellIconOverlayIdentifier COM DLL for ownCloud sync status.
//!
//! Exports five COM objects — one per sync state — each identified by a
//! fixed CLSID. Windows Explorer queries every registered overlay handler
//! via IsMemberOf and shows the highest-priority matching icon.

#![allow(non_snake_case)]

mod icons;
mod registration;

use std::sync::atomic::{AtomicI32, Ordering};
use windows::core::{implement, ComInterface, IUnknown, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, HINSTANCE, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::UI::Shell::{
    IShellIconOverlayIdentifier, IShellIconOverlayIdentifier_Impl, ISIOI_ICONFILE, ISIOI_ICONINDEX,
};

#[cfg(windows)]
use oc_ipc::PipeConnection;

// ---------------------------------------------------------------------------
// CLSIDs
// ---------------------------------------------------------------------------

pub const CLSID_OC_OVERLAY_OK: GUID = GUID {
    data1: 0xABCD_0001,
    data2: 0x1234,
    data3: 0x5678,
    data4: [0x90, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
};

pub const CLSID_OC_OVERLAY_SYNC: GUID = GUID {
    data1: 0xABCD_0002,
    data2: 0x1234,
    data3: 0x5678,
    data4: [0x90, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
};

pub const CLSID_OC_OVERLAY_WARNING: GUID = GUID {
    data1: 0xABCD_0003,
    data2: 0x1234,
    data3: 0x5678,
    data4: [0x90, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
};

pub const CLSID_OC_OVERLAY_ERROR: GUID = GUID {
    data1: 0xABCD_0004,
    data2: 0x1234,
    data3: 0x5678,
    data4: [0x90, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
};

pub const CLSID_OC_OVERLAY_EXCLUDED: GUID = GUID {
    data1: 0xABCD_0005,
    data2: 0x1234,
    data3: 0x5678,
    data4: [0x90, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
};

// ---------------------------------------------------------------------------
// DLL reference count and instance handle
// ---------------------------------------------------------------------------

static DLL_REF_COUNT: AtomicI32 = AtomicI32::new(0);

// SAFETY: Written once under the loader lock during DLL_PROCESS_ATTACH;
// read-only thereafter. Single-writer guarantee is upheld by the OS loader.
static mut DLL_HINSTANCE: HINSTANCE = HINSTANCE(0);

// ---------------------------------------------------------------------------
// DllMain
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn DllMain(
    hinstance: HINSTANCE,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        unsafe { DLL_HINSTANCE = hinstance };
    }
    1
}

// ---------------------------------------------------------------------------
// COM DLL entry points
// ---------------------------------------------------------------------------

/// # Safety
/// `ppv` must be a valid non-null out-pointer per COM contract.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    let clsid = unsafe { &*rclsid };
    let iid = unsafe { &*riid };

    let factory: IUnknown = match *clsid {
        CLSID_OC_OVERLAY_OK => ClassFactoryOk.into(),
        CLSID_OC_OVERLAY_SYNC => ClassFactorySync.into(),
        CLSID_OC_OVERLAY_WARNING => ClassFactoryWarning.into(),
        CLSID_OC_OVERLAY_ERROR => ClassFactoryError.into(),
        CLSID_OC_OVERLAY_EXCLUDED => ClassFactoryExcluded.into(),
        _ => return HRESULT(0x8004_0154_u32 as i32),
    };

    unsafe { factory.query(iid, ppv) }
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if DLL_REF_COUNT.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    match registration::register() {
        Ok(()) => S_OK,
        Err(_) => HRESULT(0x8007_0005_u32 as i32),
    }
}

#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    match registration::unregister() {
        Ok(()) => S_OK,
        Err(_) => HRESULT(0x8007_0005_u32 as i32),
    }
}

// ---------------------------------------------------------------------------
// Concrete class factories (one per overlay type — avoids RuntimeName bound)
// ---------------------------------------------------------------------------

macro_rules! impl_factory {
    ($factory:ident, $overlay:ty) => {
        #[implement(IClassFactory)]
        struct $factory;

        impl IClassFactory_Impl for $factory {
            fn CreateInstance(
                &self,
                outer: Option<&IUnknown>,
                iid: *const GUID,
                ppv: *mut *mut std::ffi::c_void,
            ) -> windows::core::Result<()> {
                if outer.is_some() {
                    return Err(CLASS_E_NOAGGREGATION.into());
                }
                DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
                let handler: IShellIconOverlayIdentifier = <$overlay>::default().into();
                // SAFETY: COM-contract pointers validated by the caller.
                unsafe { handler.query(iid, ppv).ok() }
            }

            fn LockServer(
                &self,
                lock: windows::Win32::Foundation::BOOL,
            ) -> windows::core::Result<()> {
                if lock.as_bool() {
                    DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
                } else {
                    DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
                }
                Ok(())
            }
        }
    };
}

impl_factory!(ClassFactoryOk, OcOverlayOk);
impl_factory!(ClassFactorySync, OcOverlaySync);
impl_factory!(ClassFactoryWarning, OcOverlayWarning);
impl_factory!(ClassFactoryError, OcOverlayError);
impl_factory!(ClassFactoryExcluded, OcOverlayExcluded);

// ---------------------------------------------------------------------------
// Overlay handler structs
// ---------------------------------------------------------------------------

#[implement(IShellIconOverlayIdentifier)]
#[derive(Default)]
struct OcOverlayOk;

#[implement(IShellIconOverlayIdentifier)]
#[derive(Default)]
struct OcOverlaySync;

#[implement(IShellIconOverlayIdentifier)]
#[derive(Default)]
struct OcOverlayWarning;

#[implement(IShellIconOverlayIdentifier)]
#[derive(Default)]
struct OcOverlayError;

#[implement(IShellIconOverlayIdentifier)]
#[derive(Default)]
struct OcOverlayExcluded;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Query the daemon for the sync status of `path`. Returns a `'static` str
/// tag, or `"NONE"` on any error so overlays degrade silently.
fn get_file_status(path: &str) -> &'static str {
    let result = (|| -> Result<String, oc_ipc::IpcError> {
        let mut conn = PipeConnection::connect()?;
        let response = conn.send_command(&format!("RETRIEVE_FILE_STATUS:{}", path))?;
        let tag = response
            .splitn(3, ':')
            .nth(1)
            .ok_or(oc_ipc::IpcError::InvalidResponse)?
            .to_owned();
        Ok(tag)
    })();

    match result.as_deref() {
        Ok("OK") => "OK",
        Ok("SYNC") => "SYNC",
        Ok("WARNING") => "WARNING",
        Ok("ERROR") => "ERROR",
        Ok("EXCLUDED") => "EXCLUDED",
        _ => "NONE",
    }
}

/// Convert a null-terminated wide-char pointer to a Rust `String`.
///
/// # Safety
/// `ptr` must point to a valid null-terminated UTF-16 sequence.
unsafe fn pcwstr_to_string(ptr: PCWSTR) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { ptr.to_string().ok() }
}

/// Write `text` as UTF-16 into Explorer's buffer `buf` of `cchmax` wide chars.
///
/// # Safety
/// `buf` must point to a writable buffer of at least `cchmax` wide chars.
unsafe fn write_wide_str(buf: PWSTR, cchmax: i32, text: &str) {
    let encoded: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let len = encoded.len().min(cchmax as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), buf.0, len);
    }
}

// ---------------------------------------------------------------------------
// IShellIconOverlayIdentifier_Impl — one macro covers all five structs.
//
// GetPriority returns i32 directly (windows 0.52 out-param → return-value
// conversion). Lower number = higher priority in Explorer.
// Priority: Error=1 (highest), Sync=2, Warning=3, OK=4, Excluded=5 (lowest)
// ---------------------------------------------------------------------------

macro_rules! impl_overlay {
    ($ty:ty, $tag:literal, $icon_idx:expr, $priority:expr) => {
        impl IShellIconOverlayIdentifier_Impl for $ty {
            fn IsMemberOf(
                &self,
                pwszpath: &PCWSTR,
                _dwattrib: u32,
            ) -> windows::core::Result<()> {
                // SAFETY: Explorer guarantees `pwszpath` is a valid
                // null-terminated wide string for the duration of this call.
                let path = match unsafe { pcwstr_to_string(*pwszpath) } {
                    Some(p) => p,
                    None => return Err(E_FAIL.into()),
                };
                if get_file_status(&path) == $tag {
                    Ok(())
                } else {
                    Err(E_FAIL.into())
                }
            }

            fn GetOverlayInfo(
                &self,
                pwsziconfile: PWSTR,
                cchmax: i32,
                pindex: *mut i32,
                pdwflags: *mut u32,
            ) -> windows::core::Result<()> {
                if pindex.is_null() || pdwflags.is_null() {
                    return Err(E_POINTER.into());
                }
                let mut path_buf = vec![0u16; cchmax as usize];
                unsafe {
                    // SAFETY: DLL_HINSTANCE is read-only after DllMain.
                    // HMODULE and HINSTANCE are layout-compatible (both *mut c_void).
                    GetModuleFileNameW(
                        HMODULE(DLL_HINSTANCE.0),
                        &mut path_buf,
                    );
                    write_wide_str(
                        pwsziconfile,
                        cchmax,
                        &String::from_utf16_lossy(&path_buf),
                    );
                    *pindex = $icon_idx;
                    *pdwflags = ISIOI_ICONFILE | ISIOI_ICONINDEX;
                }
                Ok(())
            }

            fn GetPriority(&self) -> windows::core::Result<i32> {
                Ok($priority)
            }
        }
    };
}

impl_overlay!(OcOverlayOk, "OK", 0, 4);
impl_overlay!(OcOverlaySync, "SYNC", 1, 2);
impl_overlay!(OcOverlayWarning, "WARNING", 2, 3);
impl_overlay!(OcOverlayError, "ERROR", 3, 1);
impl_overlay!(OcOverlayExcluded, "EXCLUDED", 4, 5);

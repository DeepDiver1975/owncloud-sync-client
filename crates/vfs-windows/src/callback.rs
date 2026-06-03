//! Hydration callback registration and dispatch.
//!
//! Windows calls into our process via the `CF_CALLBACK_TYPE_FETCH_DATA`
//! callback whenever a user or application opens a virtual file.  We forward
//! the request to the sync engine over a tokio mpsc channel.

use std::sync::Arc;

use camino::Utf8PathBuf;
use tokio::sync::mpsc;

use windows::core::HSTRING;
use windows::Win32::Storage::CloudFilters::{
    CfConnectSyncRoot, CfDisconnectSyncRoot, CF_CALLBACK_INFO, CF_CALLBACK_PARAMETERS,
    CF_CALLBACK_REGISTRATION, CF_CALLBACK_TYPE_FETCH_DATA, CF_CALLBACK_TYPE_NONE,
    CF_CONNECTION_KEY, CF_CONNECT_FLAG_NONE,
};

use crate::error::{Result, VfsWindowsError};

/// Opaque callback info needed to call `CfExecute` from the sync engine.
#[derive(Debug, Clone)]
pub struct RawCallbackInfo {
    pub connection_key: CF_CONNECTION_KEY,
    pub transfer_key: i64,
    pub request_key: i64,
}

/// A request sent to the sync engine asking it to provide file data.
#[derive(Debug)]
pub struct HydrationRequest {
    pub path: Utf8PathBuf,
    pub offset: u64,
    pub length: u64,
    pub callback_info: RawCallbackInfo,
}

/// Context shared between the callback registration and the callback function.
pub struct HydrationCallbackContext {
    pub tx: mpsc::Sender<HydrationRequest>,
}

struct CallbackState {
    ctx: Arc<HydrationCallbackContext>,
}

/// The `extern "system"` hydration callback invoked by Windows.
///
/// # Safety
///
/// This function is called by the Windows kernel via a registered callback.
/// The `callback_info` pointer is guaranteed valid for the duration of the call.
unsafe extern "system" fn fetch_data_callback(
    callback_info: *const CF_CALLBACK_INFO,
    callback_params: *const CF_CALLBACK_PARAMETERS,
) {
    // Safety: Windows guarantees callback_info is a valid pointer here.
    let info = &*callback_info;
    // Safety: same guarantee for callback_params.
    let params = &*callback_params;

    // Recover the Arc<HydrationCallbackContext> from CallbackContext.
    // Safety: we stored a raw pointer to a Box<CallbackState> in CfConnectSyncRoot.
    let state = &*(info.CallbackContext as *const CallbackState);

    let path = {
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

    // Safety: we only register CF_CALLBACK_TYPE_FETCH_DATA so this union arm is valid.
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

    // Non-blocking send — drop if channel is full; OS will retry.
    let _ = state.ctx.tx.try_send(req);
}

/// Register a `CF_CALLBACK_TYPE_FETCH_DATA` callback for the sync root at `root`.
///
/// Returns a [`CF_CONNECTION_KEY`] to pass to [`unregister_hydration_callback`].
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if `CfConnectSyncRoot` fails.
pub fn register_hydration_callback(
    root: &camino::Utf8Path,
    ctx: Arc<HydrationCallbackContext>,
) -> Result<CF_CONNECTION_KEY> {
    let state = Box::new(CallbackState { ctx });
    let state_ptr = Box::into_raw(state);

    let callbacks = [
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_FETCH_DATA,
            // In windows 0.52 `CF_CALLBACK` is a type alias for
            // `Option<unsafe extern "system" fn(..)>`, not a union.
            Callback: Some(fetch_data_callback),
        },
        // Sentinel entry required by CfConnectSyncRoot.
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: None,
        },
    ];

    let root_wide = HSTRING::from(root.as_str());

    // Safety: callbacks ends with CF_CALLBACK_TYPE_NONE; state_ptr is valid for
    // the duration of the connection; root_wide is a valid wide string.
    // CfConnectSyncRoot returns the connection key directly in 0.52.
    let connection_key = unsafe {
        CfConnectSyncRoot(
            windows::core::PCWSTR(root_wide.as_ptr()),
            callbacks.as_ptr(),
            Some(state_ptr as *const core::ffi::c_void),
            CF_CONNECT_FLAG_NONE,
        )
    }
    .map_err(|e| {
        // Safety: CfConnectSyncRoot failed so no callback was installed; reclaim.
        unsafe { drop(Box::from_raw(state_ptr)) };
        VfsWindowsError::CfApi(e)
    })?;

    Ok(connection_key)
}

/// Unregister a previously registered hydration callback.
///
/// # Errors
///
/// Returns [`VfsWindowsError::CfApi`] if `CfDisconnectSyncRoot` fails.
pub fn unregister_hydration_callback(key: CF_CONNECTION_KEY) -> Result<()> {
    // Safety: key is a valid connection key from register_hydration_callback.
    unsafe { CfDisconnectSyncRoot(key) }.map_err(VfsWindowsError::CfApi)?;
    Ok(())
}

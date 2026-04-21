# Plan 8: Shell Integration — Windows (Rust COM DLLs)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement three Windows Explorer shell extension DLLs in Rust using `windows-rs`: `oc-ipc.dll` (named pipe IPC helper), `oc-overlay.dll` (sync status overlay icons), and `oc-contextmenu.dll` (right-click context menu).

**Architecture:** Three COM in-process DLLs registered to HKCU (no elevation). `oc-ipc.dll` manages named pipe connection to `ocsyncd` socket API. `oc-overlay.dll` implements `IShellIconOverlayIdentifier` — queries file status via `oc-ipc`, maps to 5 overlay icons. `oc-contextmenu.dll` implements `IShellExtInit` + `IContextMenu3` — queries menu items, builds submenu, executes commands.

**Tech Stack:** Rust 2021, windows-rs (implement_element, com features), cdylib crate type. Must compile for `x86_64-pc-windows-msvc`. Depends on socket-api wire protocol (Plan 5) but NOT the socket-api crate — just the text protocol.

---

## Wire Protocol Reference

Socket API wire protocol (text over named pipe `\\.\pipe\ownCloud-{Username}`):
- Send: `RETRIEVE_FILE_STATUS:path\n` → receive: `STATUS:tag:path\n`
- Send: `GET_MENU_ITEMS:path\n` → receive: `GET_MENU_ITEMS:path\x1eitem_name:item_cmd:item_state\x1e...\n`
- Send: `SHARE:path\n`, `COPY_PRIVATE_LINK:path\n`, `MAKE_AVAILABLE_LOCALLY:path\n`, `MAKE_ONLINE_ONLY:path\n`
- Status tags: `OK`, `SYNC`, `WARNING`, `ERROR`, `EXCLUDED`, `NONE`
- Field separator in responses: `\x1e`

## Overlay Priority Reference

Overlay priority indices (1-5, lower = higher priority in Explorer):
- 1: Error
- 2: Syncing
- 3: Warning
- 4: Synced (OK)
- 5: Excluded

## COM DLL Export Requirements

- `DllGetClassObject` — returns class factory for registered CLSIDs
- `DllCanUnloadNow` — returns S_OK if DLL can be unloaded
- `DllRegisterServer` — registers to HKCU
- `DllUnregisterServer` — removes from HKCU

## File Map

```
shell-integration/windows/
  Cargo.toml              # workspace with 3 cdylib members
  oc-ipc/
    Cargo.toml
    src/lib.rs            # PipeConnection, send_command(), recv_line()
  oc-overlay/
    Cargo.toml
    src/lib.rs            # 5 overlay handler structs implementing IShellIconOverlayIdentifier
    src/registration.rs   # DllRegisterServer / DllUnregisterServer for overlays
    src/icons.rs          # embedded overlay icon bitmaps (PNG → HICON)
  oc-contextmenu/
    Cargo.toml
    src/lib.rs            # OcContextMenu implementing IShellExtInit + IContextMenu3
    src/registration.rs   # DllRegisterServer / DllUnregisterServer for context menu
    src/menu_builder.rs   # parse GET_MENU_ITEMS response → Win32 menu handles
```

---

## Tasks

### Task 1: Workspace Cargo.toml + oc-ipc Cargo.toml

- [ ] Create `shell-integration/windows/Cargo.toml` as a Cargo workspace:

```toml
# shell-integration/windows/Cargo.toml
[workspace]
members = ["oc-ipc", "oc-overlay", "oc-contextmenu"]
resolver = "2"
```

- [ ] Create `shell-integration/windows/oc-ipc/Cargo.toml`:

```toml
[package]
name = "oc-ipc"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]
# rlib so oc-overlay and oc-contextmenu can link against it statically

[dependencies]
thiserror = "1"

[dependencies.windows]
version = "0.52"
features = [
    "Win32_Foundation",
    "Win32_Storage_FileSystem",
    "Win32_System_Pipes",
    "Win32_System_IO",
]
```

---

### Task 2: oc-ipc/src/lib.rs — Named Pipe Client

- [ ] Create `shell-integration/windows/oc-ipc/src/lib.rs` with the full `PipeConnection` implementation:

```rust
//! oc-ipc: Named pipe client for the ownCloud sync daemon socket API.
//!
//! Connects to \\.\pipe\ownCloud-{USERNAME} and exchanges line-oriented
//! text commands with the ocsyncd process.

use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING,
};
use windows::Win32::System::IO::WriteFile;
use windows::Win32::System::Pipes::SetNamedPipeHandleState;
use windows::Win32::Storage::FileSystem::ReadFile;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::System::Pipes::PIPE_READMODE_MESSAGE;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("failed to connect to named pipe: {0}")]
    Connect(#[source] windows::core::Error),

    #[error("failed to write to named pipe: {0}")]
    Write(#[source] windows::core::Error),

    #[error("failed to read from named pipe: {0}")]
    Read(#[source] windows::core::Error),

    #[error("daemon response was not valid UTF-8 or had unexpected format")]
    InvalidResponse,

    #[error("USERNAME environment variable not set")]
    NoUsername,
}

/// A synchronous named pipe connection to the ocsyncd daemon.
///
/// One `PipeConnection` should be used per command exchange and then
/// dropped. Explorer shell extensions are called on arbitrary threads,
/// so `Send` is required and each thread must create its own connection.
pub struct PipeConnection {
    /// Raw Win32 pipe handle. Invariant: always valid while `self` is live.
    handle: HANDLE,
}

// SAFETY: HANDLE is a raw pointer, but we guarantee exclusive ownership —
// no other thread shares this handle value while the PipeConnection is live.
unsafe impl Send for PipeConnection {}

impl PipeConnection {
    /// Open a new connection to the ocsyncd named pipe.
    ///
    /// The pipe name is `\\.\pipe\ownCloud-{USERNAME}` where `USERNAME`
    /// comes from the `USERNAME` environment variable (set by Windows for
    /// every process).
    pub fn connect() -> Result<Self, IpcError> {
        let username = std::env::var("USERNAME").map_err(|_| IpcError::NoUsername)?;
        let pipe_name = format!(r"\\.\pipe\ownCloud-{}", username);

        // Encode pipe name as null-terminated UTF-16 for Win32.
        let pipe_name_wide: Vec<u16> = pipe_name
            .encode_utf16()
            .chain(std::iter::once(0u16))
            .collect();

        // SAFETY: CreateFileW requires a valid null-terminated wide string.
        // `pipe_name_wide` is correctly encoded above. All other arguments
        // are value types with no aliasing constraints.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(pipe_name_wide.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,                   // default security attributes
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,   // non-overlapped (synchronous) mode
                None,                   // no template file
            )
            .map_err(IpcError::Connect)?
        };

        if handle == INVALID_HANDLE_VALUE {
            // CreateFileW returns INVALID_HANDLE_VALUE on failure and sets
            // GetLastError; the `?` above already handles that, but be explicit.
            return Err(IpcError::Connect(windows::core::Error::from_win32()));
        }

        // Switch pipe to message-read mode so each ReadFile returns one
        // complete daemon response line rather than a partial byte stream.
        let mut mode = PIPE_READMODE_MESSAGE;
        // SAFETY: `handle` is valid (checked above). `mode` is a local u32;
        // its address is only used for the duration of this call.
        unsafe {
            SetNamedPipeHandleState(
                handle,
                Some(&mut mode),
                None, // max collection count: keep default
                None, // collect data timeout: keep default
            )
            .map_err(IpcError::Connect)?;
        }

        Ok(PipeConnection { handle })
    }

    /// Send `cmd` (without trailing newline) and return the single response
    /// line from the daemon (newline stripped).
    ///
    /// The daemon protocol is strictly one-command → one-response, so this
    /// method writes the command and immediately reads the response.
    pub fn send_command(&mut self, cmd: &str) -> Result<String, IpcError> {
        // Build the wire bytes: UTF-8 command + ASCII newline terminator.
        let mut payload = cmd.as_bytes().to_vec();
        payload.push(b'\n');

        let mut bytes_written: u32 = 0;
        // SAFETY: `self.handle` is valid for the lifetime of `self`. The
        // payload slice lives for the duration of WriteFile. `bytes_written`
        // is a local out-parameter with no aliasing risk.
        unsafe {
            WriteFile(
                self.handle,
                Some(&payload),
                Some(&mut bytes_written),
                None, // no OVERLAPPED — synchronous I/O
            )
            .map_err(IpcError::Write)?;
        }

        // Read the response into a fixed-size stack buffer. The daemon
        // guarantees responses fit in 4096 bytes.
        let mut buf = [0u8; 4096];
        let mut bytes_read: u32 = 0;
        // SAFETY: `self.handle` is valid. `buf` is a local array initialised
        // to zero; we only read `bytes_read` bytes from it after the call.
        unsafe {
            ReadFile(
                self.handle,
                Some(&mut buf),
                Some(&mut bytes_read),
                None,
            )
            .map_err(IpcError::Read)?;
        }

        // Trim the trailing newline (and any CR) before returning.
        let raw = &buf[..bytes_read as usize];
        let line = std::str::from_utf8(raw)
            .map_err(|_| IpcError::InvalidResponse)?
            .trim_end_matches(['\n', '\r'])
            .to_owned();

        Ok(line)
    }
}

impl Drop for PipeConnection {
    fn drop(&mut self) {
        // SAFETY: `self.handle` was opened by `connect()` and has not been
        // closed since. We are in the destructor so no other code can use
        // `self.handle` after this point.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Integration test — requires Windows with a running ocsyncd daemon.
    /// Run with: cargo test --ignored -- --nocapture
    #[test]
    #[ignore = "requires Windows + running ocsyncd daemon"]
    fn test_connect_and_version() {
        let mut conn = PipeConnection::connect()
            .expect("should connect to running ocsyncd");

        let response = conn
            .send_command("VERSION")
            .expect("should receive VERSION response");

        assert!(
            response.contains("VERSION:"),
            "expected VERSION: prefix in response, got: {:?}",
            response
        );
    }

    #[test]
    #[ignore = "requires Windows + running ocsyncd daemon"]
    fn test_retrieve_file_status_none_for_unknown_path() {
        let mut conn = PipeConnection::connect().expect("connect");
        // A path that will never be in a sync folder.
        let response = conn
            .send_command(r"RETRIEVE_FILE_STATUS:C:\does-not-exist\file.txt")
            .expect("send_command");
        // Daemon returns STATUS:NONE:path for unknown paths.
        assert!(response.starts_with("STATUS:"), "got: {:?}", response);
    }
}
```

---

### Task 3: oc-overlay — DLL Exports and Class Factory

- [ ] Create `shell-integration/windows/oc-overlay/Cargo.toml`:

```toml
[package]
name = "oc-overlay"
version = "0.1.0"
edition = "2021"

[lib]
name = "oc_overlay"
crate-type = ["cdylib"]

[dependencies]
oc-ipc = { path = "../oc-ipc" }

[dependencies.windows]
version = "0.52"
features = [
    "Win32_Foundation",
    "Win32_System_Com",
    "Win32_System_Ole",
    "Win32_System_Registry",
    "Win32_UI_Shell",
    "Win32_UI_Shell_Common",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Storage_FileSystem",
    "Win32_Graphics_Gdi",
    "implement",
]
```

- [ ] Create `shell-integration/windows/oc-overlay/src/lib.rs` with all five CLSID constants, a global `DLL_MODULE` handle, a `DllMain` entry-point, and the four required COM DLL exports. Include a `ClassFactory<T>` generic struct that implements `IClassFactory` and is specialised for each of the five overlay types:

```rust
//! oc-overlay: IShellIconOverlayIdentifier COM DLL for ownCloud sync status.
//!
//! Exports five COM objects — one per sync state — each identified by a
//! fixed CLSID.  Windows Explorer queries every registered overlay handler
//! via IsMemberOf and shows the highest-priority matching icon.

#![allow(non_snake_case)]

mod icons;
mod registration;

use std::sync::atomic::{AtomicI32, Ordering};
use windows::core::{implement, IUnknown, IUnknownImpl, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CLASS_E_NOAGGREGATION, E_FAIL, E_NOINTERFACE, E_POINTER, HINSTANCE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::UI::Shell::{
    IShellIconOverlayIdentifier, IShellIconOverlayIdentifier_Impl, ISIOI_ICONFILE, ISIOI_ICONINDEX,
};
use windows::Win32::Storage::FileSystem::GetModuleFileNameW;

use oc_ipc::PipeConnection;

// ---------------------------------------------------------------------------
// CLSIDs — one per overlay state, registered in HKCU by DllRegisterServer.
// These UUIDs are stable and must match the registry values written by
// registration.rs.
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
// DLL reference count — incremented by class factories, decremented on drop.
// DllCanUnloadNow returns S_OK when this reaches zero.
// ---------------------------------------------------------------------------
static DLL_REF_COUNT: AtomicI32 = AtomicI32::new(0);

/// Module handle stored by DllMain; needed by GetOverlayInfo to report the
/// DLL path to Explorer so it can extract the icon resource.
static mut DLL_HINSTANCE: HINSTANCE = HINSTANCE(0);

// ---------------------------------------------------------------------------
// DllMain — required for in-process COM servers so we can capture HINSTANCE.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn DllMain(
    hinstance: HINSTANCE,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        // SAFETY: DllMain is called by the OS loader under the loader lock.
        // No other Rust code runs concurrently at this point, and we only
        // write to this static once.
        unsafe { DLL_HINSTANCE = hinstance };
    }
    1 // TRUE
}

// ---------------------------------------------------------------------------
// COM DLL entry points
// ---------------------------------------------------------------------------

/// Called by COM to obtain the class factory for a given CLSID.
///
/// # Safety
/// `ppv` must be a valid non-null out-pointer. This is a COM contract
/// guaranteed by the caller (Explorer / CoCreateInstance).
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    // SAFETY: caller guarantees these pointers are valid for reads.
    let clsid = unsafe { &*rclsid };
    let iid = unsafe { &*riid };

    // Dispatch to the matching class factory.
    let factory: windows::core::IUnknown = match *clsid {
        CLSID_OC_OVERLAY_OK => {
            ClassFactory::<OcOverlayOk>::new().into()
        }
        CLSID_OC_OVERLAY_SYNC => {
            ClassFactory::<OcOverlaySync>::new().into()
        }
        CLSID_OC_OVERLAY_WARNING => {
            ClassFactory::<OcOverlayWarning>::new().into()
        }
        CLSID_OC_OVERLAY_ERROR => {
            ClassFactory::<OcOverlayError>::new().into()
        }
        CLSID_OC_OVERLAY_EXCLUDED => {
            ClassFactory::<OcOverlayExcluded>::new().into()
        }
        _ => return HRESULT(0x8004_0154_u32 as i32), // CLASS_E_CLASSNOTAVAILABLE
    };

    // QueryInterface for the requested IID (usually IClassFactory).
    factory.query(iid, ppv)
}

/// Returns S_OK when the DLL reference count is zero and COM may unload us.
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if DLL_REF_COUNT.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

/// Writes HKCU registry keys that tell Explorer about this DLL's overlays.
#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    match registration::register() {
        Ok(()) => S_OK,
        Err(_) => HRESULT(0x8007_0005_u32 as i32), // E_ACCESSDENIED
    }
}

/// Removes the HKCU registry keys written by DllRegisterServer.
#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    match registration::unregister() {
        Ok(()) => S_OK,
        Err(_) => HRESULT(0x8007_0005_u32 as i32),
    }
}

// ---------------------------------------------------------------------------
// Generic class factory
// ---------------------------------------------------------------------------

/// A COM class factory that creates instances of overlay handler `T`.
///
/// `T` must implement `IShellIconOverlayIdentifier` via `#[implement]` and
/// also provide `Default` for construction.
#[implement(IClassFactory)]
struct ClassFactory<T: Default + 'static + windows::core::RuntimeName> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Default + 'static + windows::core::RuntimeName> ClassFactory<T> {
    fn new() -> Self {
        DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
        ClassFactory {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> Drop for ClassFactory<T>
where
    T: Default + 'static + windows::core::RuntimeName,
{
    fn drop(&mut self) {
        DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl<T> IClassFactory_Impl for ClassFactory<T>
where
    T: Default
        + 'static
        + windows::core::RuntimeName
        + IShellIconOverlayIdentifier_Impl
        + IUnknownImpl,
{
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        iid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> windows::core::Result<()> {
        // COM aggregation is not supported.
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        // Construct the handler and return the requested interface.
        let handler: IShellIconOverlayIdentifier = T::default().into();
        // SAFETY: `iid` and `ppv` are COM-contract pointers validated by the
        // COM runtime before it calls CreateInstance.
        unsafe { handler.query(iid, ppv).ok() }
    }

    fn LockServer(&self, lock: windows::Win32::Foundation::BOOL) -> windows::core::Result<()> {
        if lock.as_bool() {
            DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
        } else {
            DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Overlay handler structs — one per sync state
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
// Shared helper
// ---------------------------------------------------------------------------

/// Query the daemon for the sync status of `path`.
///
/// Returns the status tag (`OK`, `SYNC`, `WARNING`, `ERROR`, `EXCLUDED`,
/// `NONE`) or `"NONE"` on any error so overlays degrade silently.
fn get_file_status(path: &str) -> &'static str {
    let result = (|| -> Result<String, oc_ipc::IpcError> {
        let mut conn = PipeConnection::connect()?;
        let response = conn.send_command(&format!("RETRIEVE_FILE_STATUS:{}", path))?;
        // Response format: STATUS:tag:path
        let tag = response
            .splitn(3, ':')
            .nth(1)
            .ok_or(oc_ipc::IpcError::InvalidResponse)?
            .to_owned();
        Ok(tag)
    })();

    match result.as_deref() {
        Ok("OK")       => "OK",
        Ok("SYNC")     => "SYNC",
        Ok("WARNING")  => "WARNING",
        Ok("ERROR")    => "ERROR",
        Ok("EXCLUDED") => "EXCLUDED",
        _              => "NONE",
    }
}

/// Convert a null-terminated wide-char pointer to a Rust `String`.
///
/// # Safety
/// `ptr` must point to a valid null-terminated UTF-16 sequence for the
/// duration of this call.
unsafe fn pcwstr_to_string(ptr: PCWSTR) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` is a valid null-terminated wide string.
    unsafe { ptr.to_string().ok() }
}

/// Write `text` as UTF-16 into the Explorer-supplied buffer `buf` of `cchmax`
/// wide chars (including null terminator).
///
/// # Safety
/// `buf` must point to a writable buffer of at least `cchmax` wide chars.
unsafe fn write_wide_str(buf: PWSTR, cchmax: i32, text: &str) {
    let encoded: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let len = encoded.len().min(cchmax as usize);
    // SAFETY: caller guarantees `buf` is valid for `cchmax` wide chars.
    unsafe {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), buf.0, len);
    }
}

/// Macro to reduce boilerplate: implement `IShellIconOverlayIdentifier_Impl`
/// for an overlay struct given the expected status tag, icon resource index,
/// and Explorer priority.
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
                    Ok(()) // S_OK — this overlay applies
                } else {
                    Err(E_FAIL.into()) // S_FALSE-equivalent: not a member
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
                // Retrieve the full path of this DLL so Explorer can extract
                // the embedded icon resource.
                let mut path_buf = vec![0u16; cchmax as usize];
                // SAFETY: `DLL_HINSTANCE` was written once during DllMain
                // under the loader lock and is read-only afterwards.
                // `path_buf` is a valid mutable slice of `cchmax` wide chars.
                unsafe {
                    GetModuleFileNameW(DLL_HINSTANCE, &mut path_buf);
                    // `pwsziconfile` is the buffer Explorer allocated; its
                    // size is `cchmax` wide chars.
                    write_wide_str(pwsziconfile, cchmax, &String::from_utf16_lossy(&path_buf));
                    *pindex = $icon_idx;
                    *pdwflags = ISIOI_ICONFILE | ISIOI_ICONINDEX;
                }
                Ok(())
            }

            fn GetPriority(
                &self,
                ppriority: *mut i32,
            ) -> windows::core::Result<()> {
                if ppriority.is_null() {
                    return Err(E_POINTER.into());
                }
                // SAFETY: `ppriority` is a valid out-pointer per the COM
                // contract; Explorer never passes null for this parameter.
                unsafe { *ppriority = $priority };
                Ok(())
            }
        }
    };
}

// Priority 4 = Synced OK (lower number = shown first when multiple overlays match)
impl_overlay!(OcOverlayOk,       "OK",       0, 4);
// Priority 2 = Syncing (high visibility for in-progress state)
impl_overlay!(OcOverlaySync,     "SYNC",     1, 2);
// Priority 3 = Warning
impl_overlay!(OcOverlayWarning,  "WARNING",  2, 3);
// Priority 1 = Error (highest priority — must always be visible)
impl_overlay!(OcOverlayError,    "ERROR",    3, 1);
// Priority 5 = Excluded (lowest priority)
impl_overlay!(OcOverlayExcluded, "EXCLUDED", 4, 5);
```

---

### Task 4: oc-overlay — IShellIconOverlayIdentifier (all five overlays)

Task 3 already defines all five overlay structs and their `IShellIconOverlayIdentifier_Impl` via the `impl_overlay!` macro. This task documents the design decisions:

- [ ] Verify that each overlay struct is `Default + Send + Sync` (required by `#[implement]`).
- [ ] Confirm icon resource indices 0–4 match the order icons are embedded in `icons.rs` (Task 5).
- [ ] Confirm Explorer priority mapping:

| Overlay     | Status tag  | Icon index | Explorer priority |
|-------------|-------------|------------|-------------------|
| OcOverlayError    | ERROR    | 3          | 1 (highest)       |
| OcOverlaySync     | SYNC     | 1          | 2                 |
| OcOverlayWarning  | WARNING  | 2          | 3                 |
| OcOverlayOk       | OK       | 0          | 4                 |
| OcOverlayExcluded | EXCLUDED | 4          | 5 (lowest)        |

- [ ] Write unit tests for `get_file_status` by shimming the IPC layer with a mock (see test module at bottom of `lib.rs`).

---

### Task 5: oc-overlay — icons.rs and registration.rs

- [ ] Create `shell-integration/windows/oc-overlay/src/icons.rs`:

```rust
//! icons.rs — Embed overlay icon PNGs at compile time and create HICONs.
//!
//! Each PNG is a 16x16 RGBA image embedded via include_bytes!.  On first use
//! each icon is loaded from the byte slice using CreateIconFromResourceEx and
//! cached in a static OnceLock.  Placeholder PNGs (minimal 1×1 pixel) are
//! used during development; replace with real artwork before shipping.

use std::sync::OnceLock;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, HICON, IMAGE_ICON, LR_DEFAULTCOLOR,
};

// Minimal valid 1×1 RGBA PNG — 67 bytes.
// xxd -i of a 1x1 transparent PNG generated by:
//   python3 -c "import zlib,struct; ..."
// Replace these with real 16×16 artwork for production builds.
const PLACEHOLDER_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG signature
    0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // width=1, height=1
    0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // bit depth=8, color=RGB
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, // IDAT chunk
    0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, // compressed pixel data
    0x00, 0x00, 0x02, 0x00, 0x01, 0xe2, 0x21, 0xbc,
    0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, // IEND chunk
    0x44, 0xae, 0x42, 0x60, 0x82,
];

// Embed actual icon files — replace PLACEHOLDER_PNG with include_bytes!
// once real artwork is available:
//   const ICON_OK_PNG:       &[u8] = include_bytes!("../icons/ok.png");
//   const ICON_SYNC_PNG:     &[u8] = include_bytes!("../icons/sync.png");
//   const ICON_WARNING_PNG:  &[u8] = include_bytes!("../icons/warning.png");
//   const ICON_ERROR_PNG:    &[u8] = include_bytes!("../icons/error.png");
//   const ICON_EXCLUDED_PNG: &[u8] = include_bytes!("../icons/excluded.png");
const ICON_OK_PNG:       &[u8] = PLACEHOLDER_PNG;
const ICON_SYNC_PNG:     &[u8] = PLACEHOLDER_PNG;
const ICON_WARNING_PNG:  &[u8] = PLACEHOLDER_PNG;
const ICON_ERROR_PNG:    &[u8] = PLACEHOLDER_PNG;
const ICON_EXCLUDED_PNG: &[u8] = PLACEHOLDER_PNG;

/// Load a PNG byte slice as a Win32 HICON using CreateIconFromResourceEx.
///
/// `bytes` must be a valid PNG image.  Returns `None` if the Win32 call
/// fails (e.g. the byte slice is corrupt or the OS cannot load the image).
///
/// # Safety
/// `bytes` must remain valid for the lifetime of the returned HICON.  Since
/// we only call this with `'static` slices the invariant is always satisfied.
fn load_icon_from_bytes(bytes: &'static [u8]) -> Option<HICON> {
    // SAFETY: `bytes` is a 'static slice pointing to embedded PNG data.
    // CreateIconFromResourceEx reads from this pointer synchronously and
    // does not retain a reference after the call returns.
    // The uWidth/uHeight of 0 ask the OS to use the natural image size.
    let icon = unsafe {
        CreateIconFromResourceEx(
            bytes,
            true,        // fIcon = TRUE (icon, not cursor)
            0x0003_0000, // dwVer = 0x00030000 (version 3.0)
            0,           // cxDesired = 0 → natural width
            0,           // cyDesired = 0 → natural height
            LR_DEFAULTCOLOR,
        )
        .ok()?
    };
    Some(icon)
}

/// Returns the cached HICON for the "OK / Synced" state.
pub fn icon_ok() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_OK_PNG))
}

/// Returns the cached HICON for the "Syncing" state.
pub fn icon_sync() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_SYNC_PNG))
}

/// Returns the cached HICON for the "Warning" state.
pub fn icon_warning() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_WARNING_PNG))
}

/// Returns the cached HICON for the "Error" state.
pub fn icon_error() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_ERROR_PNG))
}

/// Returns the cached HICON for the "Excluded" state.
pub fn icon_excluded() -> Option<HICON> {
    static CACHE: OnceLock<Option<HICON>> = OnceLock::new();
    *CACHE.get_or_init(|| load_icon_from_bytes(ICON_EXCLUDED_PNG))
}
```

- [ ] Create `shell-integration/windows/oc-overlay/src/registration.rs`:

```rust
//! registration.rs — DllRegisterServer / DllUnregisterServer for overlays.
//!
//! Writes (or deletes) keys under:
//!   HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\
//!       ShellIconOverlayIdentifiers\ownCloud{Name}
//!
//! Each key's default value is the CLSID string, e.g.
//!   "{ABCD0001-1234-5678-90AB-CDEF01234567}"
//!
//! No elevation is required because we write to HKCU.

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCreateKeyExW, RegDeleteKeyW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::Foundation::ERROR_SUCCESS;

use crate::{
    CLSID_OC_OVERLAY_ERROR, CLSID_OC_OVERLAY_EXCLUDED, CLSID_OC_OVERLAY_OK,
    CLSID_OC_OVERLAY_SYNC, CLSID_OC_OVERLAY_WARNING,
};
use windows::core::GUID;

const BASE_KEY: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\
     \\ShellIconOverlayIdentifiers";

/// Format a GUID as the registry string `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`.
fn guid_to_string(g: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1,
        g.data2,
        g.data3,
        g.data4[0],
        g.data4[1],
        g.data4[2],
        g.data4[3],
        g.data4[4],
        g.data4[5],
        g.data4[6],
        g.data4[7],
    )
}

/// Open or create a registry key under HKCU and set its default value.
///
/// # Safety
/// `key_path` must be a valid null-terminated wide string.
fn write_reg_key(subkey: &str, value: &str) -> Result<(), windows::core::Error> {
    let full_path: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let mut hkey = HKEY::default();

    // SAFETY: `full_path` is a valid null-terminated UTF-16 string.
    // `hkey` is an out-parameter; we close it after use.
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(full_path.as_ptr()),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(windows::core::Error::from_win32());
    }

    // Write the CLSID string as the key's default (unnamed) value.
    let value_wide: Vec<u8> = value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|c| c.to_le_bytes())
        .collect();

    // SAFETY: `hkey` is a valid open registry key.  `value_wide` is a
    // properly encoded REG_SZ byte array including the null terminator.
    let result2 = unsafe {
        RegSetValueExW(
            hkey,
            PCWSTR::null(), // default value
            0,
            REG_SZ,
            Some(&value_wide),
        )
    };

    // SAFETY: `hkey` must be closed to release the OS handle.
    // We do this regardless of whether SetValue succeeded.
    unsafe { windows::Win32::System::Registry::RegCloseKey(hkey) };

    if result2 != ERROR_SUCCESS {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

/// Register all five overlay handlers in HKCU.
pub fn register() -> Result<(), windows::core::Error> {
    let overlays = [
        ("ownCloudOK",       &CLSID_OC_OVERLAY_OK),
        ("ownCloudSync",     &CLSID_OC_OVERLAY_SYNC),
        ("ownCloudWarning",  &CLSID_OC_OVERLAY_WARNING),
        ("ownCloudError",    &CLSID_OC_OVERLAY_ERROR),
        ("ownCloudExcluded", &CLSID_OC_OVERLAY_EXCLUDED),
    ];

    for (name, clsid) in &overlays {
        let subkey = format!("{}\\{}", BASE_KEY, name);
        write_reg_key(&subkey, &guid_to_string(clsid))?;
    }
    Ok(())
}

/// Unregister all five overlay handlers from HKCU.
pub fn unregister() -> Result<(), windows::core::Error> {
    let names = [
        "ownCloudOK",
        "ownCloudSync",
        "ownCloudWarning",
        "ownCloudError",
        "ownCloudExcluded",
    ];

    for name in &names {
        let subkey = format!("{}\\{}", BASE_KEY, name);
        let subkey_wide: Vec<u16> =
            subkey.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `subkey_wide` is a valid null-terminated wide string.
        // RegDeleteKeyW under HKCU requires no elevation.
        // Ignore errors for non-existent keys (already unregistered).
        unsafe {
            let _ = RegDeleteKeyW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey_wide.as_ptr()),
            );
        }
    }
    Ok(())
}
```

---

### Task 6: oc-contextmenu Cargo.toml + OcContextMenu Struct

- [ ] Create `shell-integration/windows/oc-contextmenu/Cargo.toml`:

```toml
[package]
name = "oc-contextmenu"
version = "0.1.0"
edition = "2021"

[lib]
name = "oc_contextmenu"
crate-type = ["cdylib"]

[dependencies]
oc-ipc = { path = "../oc-ipc" }

[dependencies.windows]
version = "0.52"
features = [
    "Win32_Foundation",
    "Win32_System_Com",
    "Win32_System_Ole",
    "Win32_System_Registry",
    "Win32_UI_Shell",
    "Win32_UI_Shell_Common",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Storage_FileSystem",
    "implement",
]
```

- [ ] Create `shell-integration/windows/oc-contextmenu/src/lib.rs` — define the `OcContextMenu` struct and its CLSID, plus the four DLL exports:

```rust
//! oc-contextmenu: IContextMenu3 COM DLL for ownCloud right-click integration.
//!
//! Registers under HKCU\...\*\shellex\ContextMenuHandlers\ownCloud so that
//! Explorer adds an "ownCloud" submenu to the right-click menu of any file.
//! Menu items are fetched live from the daemon via GET_MENU_ITEMS.

#![allow(non_snake_case)]

mod menu_builder;
mod registration;

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use windows::core::{implement, IUnknown, GUID, HRESULT, PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, HINSTANCE, HWND, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl, IDataObject};
use windows::Win32::System::Registry::HKEY;
use windows::Win32::UI::Shell::{
    IContextMenu, IContextMenu2, IContextMenu3, IContextMenu3_Impl, IContextMenu2_Impl,
    IContextMenu_Impl, IShellExtInit, IShellExtInit_Impl, CMINVOKECOMMANDINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, InsertMenuW, HMENU, MF_BYPOSITION, MF_GRAYED,
    MF_POPUP, MF_SEPARATOR, MF_STRING,
};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Shell::DragQueryFileW;

use oc_ipc::PipeConnection;
use menu_builder::{parse_menu_items, MenuItemDef};

// ---------------------------------------------------------------------------
// CLSID for the context menu handler.
// ---------------------------------------------------------------------------

pub const CLSID_OC_CONTEXT_MENU: GUID = GUID {
    data1: 0xABCD_0010,
    data2: 0x1234,
    data3: 0x5678,
    data4: [0x90, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
};

// DLL reference count and module handle (same pattern as oc-overlay).
static DLL_REF_COUNT: AtomicI32 = AtomicI32::new(0);
static mut DLL_HINSTANCE: HINSTANCE = HINSTANCE(0);

#[no_mangle]
pub extern "system" fn DllMain(
    hinstance: HINSTANCE,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        // SAFETY: Written once under the loader lock during DLL attach.
        unsafe { DLL_HINSTANCE = hinstance };
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM guarantees these pointers are valid for reads.
    let clsid = unsafe { &*rclsid };
    let iid = unsafe { &*riid };

    if *clsid != CLSID_OC_CONTEXT_MENU {
        return HRESULT(0x8004_0154_u32 as i32); // CLASS_E_CLASSNOTAVAILABLE
    }

    let factory: IUnknown = ContextMenuFactory::new().into();
    factory.query(iid, ppv)
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
// Class factory
// ---------------------------------------------------------------------------

#[implement(IClassFactory)]
struct ContextMenuFactory;

impl ContextMenuFactory {
    fn new() -> Self {
        DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
        ContextMenuFactory
    }
}

impl Drop for ContextMenuFactory {
    fn drop(&mut self) {
        DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IClassFactory_Impl for ContextMenuFactory {
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        iid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> windows::core::Result<()> {
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let handler: IShellExtInit = OcContextMenu::new().into();
        // SAFETY: COM-contract pointers validated by the runtime.
        unsafe { handler.query(iid, ppv).ok() }
    }

    fn LockServer(&self, lock: windows::Win32::Foundation::BOOL) -> windows::core::Result<()> {
        if lock.as_bool() {
            DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
        } else {
            DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Context menu COM object
// ---------------------------------------------------------------------------

/// COM object implementing IShellExtInit + IContextMenu + IContextMenu2 +
/// IContextMenu3 for the ownCloud right-click menu.
#[implement(IShellExtInit, IContextMenu, IContextMenu2, IContextMenu3)]
pub struct OcContextMenu {
    /// File paths selected by the user in Explorer (from IDataObject).
    selected_paths: Mutex<Vec<String>>,
    /// Menu items fetched from ocsyncd for the first selected path.
    menu_items: Mutex<Vec<MenuItemDef>>,
}

impl OcContextMenu {
    pub fn new() -> Self {
        DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
        OcContextMenu {
            selected_paths: Mutex::new(Vec::new()),
            menu_items: Mutex::new(Vec::new()),
        }
    }
}

impl Drop for OcContextMenu {
    fn drop(&mut self) {
        DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}
```

---

### Task 7: oc-contextmenu — IShellExtInit::Initialize

- [ ] Add the `IShellExtInit_Impl` block to `oc-contextmenu/src/lib.rs`:

```rust
impl IShellExtInit_Impl for OcContextMenu {
    /// Called by Explorer with the list of selected files.
    ///
    /// Stores the paths and pre-fetches menu items for the first path.
    fn Initialize(
        &self,
        _pidlfolder: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST,
        pdataobj: Option<&IDataObject>,
        _hkeyprogid: HKEY,
    ) -> windows::core::Result<()> {
        let data_obj = pdataobj.ok_or(E_FAIL)?;

        // Request CF_HDROP format from the IDataObject.
        let format_etc = windows::Win32::System::Com::FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: windows::Win32::System::Com::DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: windows::Win32::System::Com::TYMED_HGLOBAL.0 as u32,
        };
        // SAFETY: GetData returns an owned STGMEDIUM; we must call
        // ReleaseStgMedium when finished to avoid leaking the HGLOBAL.
        let medium = unsafe { data_obj.GetData(&format_etc)? };

        let hdrop = windows::Win32::UI::Shell::HDROP(medium.Anonymous.hGlobal.0 as isize);

        // DragQueryFileW with index 0xFFFFFFFF returns the file count.
        // SAFETY: `hdrop` is a valid HDROP obtained from the STGMEDIUM above.
        let count = unsafe { DragQueryFileW(hdrop, 0xFFFF_FFFF, None) };

        let mut paths: Vec<String> = Vec::with_capacity(count as usize);
        for i in 0..count {
            // First call: get required buffer length (returned value excludes null).
            // SAFETY: `hdrop` is valid; passing null with 0 size queries the length.
            let len = unsafe { DragQueryFileW(hdrop, i, None) } as usize + 1;
            let mut buf = vec![0u16; len];
            // SAFETY: `buf` is a mutable slice of the correct length.
            unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };
            let path = String::from_utf16_lossy(&buf[..len - 1]).to_string();
            paths.push(path);
        }

        // Release the STGMEDIUM to free the HGLOBAL.
        // SAFETY: `medium` was returned by GetData and must be released exactly once.
        unsafe {
            windows::Win32::System::Com::ReleaseStgMedium(
                &medium as *const _ as *mut _,
            );
        }

        // Fetch menu items for the first selected path.
        let menu_items = if let Some(first_path) = paths.first() {
            PipeConnection::connect()
                .and_then(|mut conn| conn.send_command(&format!("GET_MENU_ITEMS:{}", first_path)))
                .map(|response| parse_menu_items(&response))
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        *self.selected_paths.lock().unwrap() = paths;
        *self.menu_items.lock().unwrap() = menu_items;

        Ok(())
    }
}
```

---

### Task 8: oc-contextmenu — IContextMenu, IContextMenu2, IContextMenu3

- [ ] Add `IContextMenu_Impl`, `IContextMenu2_Impl`, and `IContextMenu3_Impl` blocks to `oc-contextmenu/src/lib.rs`:

```rust
impl IContextMenu_Impl for OcContextMenu {
    /// Build and insert the ownCloud submenu into Explorer's context menu.
    fn QueryContextMenu(
        &self,
        hmenu: HMENU,
        indexmenu: u32,
        idcmdfirst: u32,
        _idcmdlast: u32,
        _uflags: u32,
    ) -> windows::core::Result<()> {
        let items = self.menu_items.lock().unwrap();
        if items.is_empty() {
            return Ok(());
        }

        // Create a popup submenu and populate it with the daemon's items.
        // SAFETY: CreatePopupMenu returns a new menu handle; we own it and
        // must either attach it to a parent menu (which transfers ownership
        // to Windows) or destroy it ourselves on failure.
        let submenu = unsafe { CreatePopupMenu()? };

        for (i, item) in items.iter().enumerate() {
            let label_wide: Vec<u16> = item
                .label
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let flags = MF_STRING
                | if item.enabled { Default::default() } else { MF_GRAYED };
            // SAFETY: `submenu` is a valid HMENU. `label_wide` is a valid
            // null-terminated wide string valid for the duration of AppendMenuW.
            let ok = unsafe {
                AppendMenuW(
                    submenu,
                    flags,
                    idcmdfirst as usize + i,
                    PCWSTR(label_wide.as_ptr()),
                )
            };
            if ok.is_err() {
                // SAFETY: Must destroy the submenu if we cannot populate it
                // completely, to avoid a handle leak.
                unsafe { let _ = DestroyMenu(submenu); }
                return Err(E_FAIL.into());
            }
        }

        // Insert a separator and the ownCloud submenu into Explorer's menu.
        // SAFETY: `hmenu` is Explorer's popup menu handle, valid for the
        // duration of QueryContextMenu. `submenu` ownership transfers to
        // `hmenu` on successful InsertMenuW with MF_POPUP.
        unsafe {
            let sep_label: Vec<u16> = vec![0u16];
            InsertMenuW(
                hmenu,
                indexmenu,
                MF_BYPOSITION | MF_SEPARATOR,
                0,
                PCWSTR(sep_label.as_ptr()),
            )?;

            let submenu_label: Vec<u16> = "ownCloud"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            InsertMenuW(
                hmenu,
                indexmenu + 1,
                MF_BYPOSITION | MF_POPUP,
                submenu.0 as usize,
                PCWSTR(submenu_label.as_ptr()),
            )?;
        }

        // Return the number of menu items added (for ID offset accounting).
        // HRESULT with items.len() in the low word signals item count to Explorer.
        Ok(())
    }

    /// Returns a verb string or help string for a given command ID.
    fn GetCommandString(
        &self,
        idcmd: usize,
        utype: u32,
        _preserved: *const u32,
        pszname: PSTR,
        cchmax: u32,
    ) -> windows::core::Result<()> {
        const GCS_VERBW: u32 = 0x0000_0004;
        const GCS_HELPTEXTW: u32 = 0x0000_0005;

        let items = self.menu_items.lock().unwrap();
        let item = items.get(idcmd).ok_or(E_FAIL)?;

        if utype == GCS_VERBW || utype == GCS_HELPTEXTW {
            let text = if utype == GCS_VERBW {
                &item.command
            } else {
                &item.label
            };
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let len = wide.len().min(cchmax as usize);
            // SAFETY: `pszname` is actually a PWSTR (wide) buffer of `cchmax`
            // chars allocated by Explorer when utype is a W variant.
            // The cast is correct because GetCommandString uses PWSTR for W variants.
            unsafe {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), pszname.0 as *mut u16, len);
            }
        }
        Ok(())
    }

    /// Execute the selected command.
    fn InvokeCommand(
        &self,
        pici: *const CMINVOKECOMMANDINFO,
    ) -> windows::core::Result<()> {
        if pici.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: `pici` is a valid pointer guaranteed by Explorer.
        let ici = unsafe { &*pici };

        // When lpVerb's high word is 0 it encodes a numeric command offset.
        // IS_INTRESOURCE checks this: high word == 0 → integer resource.
        let cmd_id = ici.lpVerb.0 as usize;
        if ici.lpVerb.0 as usize > 0xFFFF {
            // String verb — not supported in this implementation.
            return Err(E_FAIL.into());
        }

        let items = self.menu_items.lock().unwrap();
        let item = items.get(cmd_id).ok_or(E_FAIL)?;
        let command = item.command.clone();
        drop(items);

        let paths = self.selected_paths.lock().unwrap();
        let first_path = paths.first().cloned().unwrap_or_default();
        drop(paths);

        // Send the command to the daemon. Commands that act on a path use
        // the format "COMMAND:path\n".
        let wire = format!("{}:{}", command, first_path);
        PipeConnection::connect()
            .and_then(|mut conn| conn.send_command(&wire))
            .map_err(|_| E_FAIL)?;

        Ok(())
    }
}

/// IContextMenu2 extends IContextMenu with `HandleMenuMsg` for owner-draw.
/// We forward to the base implementation (no custom drawing).
impl IContextMenu2_Impl for OcContextMenu {
    fn HandleMenuMsg(&self, _umsg: u32, _wparam: usize, _lparam: isize) -> windows::core::Result<()> {
        Ok(())
    }
}

/// IContextMenu3 extends IContextMenu2 with `HandleMenuMsg2` for subclassing.
impl IContextMenu3_Impl for OcContextMenu {
    fn HandleMenuMsg2(
        &self,
        _umsg: u32,
        _wparam: usize,
        _lparam: isize,
        _plresult: *mut isize,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}
```

---

### Task 9: menu_builder.rs + registration.rs + Build and Test Guide

- [ ] Create `shell-integration/windows/oc-contextmenu/src/menu_builder.rs`:

```rust
//! menu_builder.rs — Parse the GET_MENU_ITEMS daemon response.
//!
//! Wire format:
//!   GET_MENU_ITEMS:path\x1eitem_name:item_cmd:item_state\x1e...\n
//!
//! Fields within each item are colon-separated; records are separated by
//! the ASCII unit separator `\x1e` (0x1E).

/// A single menu item definition parsed from the daemon response.
#[derive(Debug, Clone)]
pub struct MenuItemDef {
    /// Sequential ID used to match InvokeCommand's idCmd offset.
    pub id: u32,
    /// Human-readable label shown in the submenu.
    pub label: String,
    /// Wire command sent to the daemon on invocation (e.g. "SHARE").
    pub command: String,
    /// Whether the item should be clickable (false → shown greyed out).
    pub enabled: bool,
}

/// Parse a raw GET_MENU_ITEMS response into a list of `MenuItemDef`s.
///
/// Returns an empty `Vec` on any parse error so the caller degrades
/// gracefully.
pub fn parse_menu_items(response: &str) -> Vec<MenuItemDef> {
    // Strip the leading "GET_MENU_ITEMS:path\x1e" prefix.
    // The first \x1e separates the header from the first item.
    let mut parts = response.splitn(2, '\x1e');
    let _header = parts.next(); // "GET_MENU_ITEMS:path" — discarded
    let rest = match parts.next() {
        Some(r) => r,
        None => return Vec::new(),
    };

    rest.split('\x1e')
        .filter(|s| !s.is_empty())
        .enumerate()
        .filter_map(|(i, record)| {
            // Each record: "item_name:item_cmd:item_state"
            let mut fields = record.splitn(3, ':');
            let label = fields.next()?.to_owned();
            let command = fields.next()?.to_owned();
            let state = fields.next().unwrap_or("enabled");
            let enabled = state != "disabled";
            Some(MenuItemDef {
                id: i as u32,
                label,
                command,
                enabled,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_response() {
        assert!(parse_menu_items("").is_empty());
    }

    #[test]
    fn test_parse_single_item() {
        let response = "GET_MENU_ITEMS:C:\\foo\x1eShare:SHARE:enabled\x1e";
        let items = parse_menu_items(response);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Share");
        assert_eq!(items[0].command, "SHARE");
        assert!(items[0].enabled);
        assert_eq!(items[0].id, 0);
    }

    #[test]
    fn test_parse_multiple_items_with_disabled() {
        let response = concat!(
            "GET_MENU_ITEMS:C:\\foo\x1e",
            "Share:SHARE:enabled\x1e",
            "Copy private link:COPY_PRIVATE_LINK:enabled\x1e",
            "Make available locally:MAKE_AVAILABLE_LOCALLY:disabled\x1e",
        );
        let items = parse_menu_items(response);
        assert_eq!(items.len(), 3);
        assert!(items[0].enabled);
        assert!(items[1].enabled);
        assert!(!items[2].enabled);
        assert_eq!(items[2].command, "MAKE_AVAILABLE_LOCALLY");
    }

    #[test]
    fn test_parse_malformed_record_skipped() {
        // A record with no colon separators should be skipped.
        let response = "GET_MENU_ITEMS:C:\\foo\x1eSHARE\x1eShare:SHARE:enabled\x1e";
        let items = parse_menu_items(response);
        // Only the well-formed item should survive.
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Share");
    }
}
```

- [ ] Create `shell-integration/windows/oc-contextmenu/src/registration.rs`:

```rust
//! registration.rs — DllRegisterServer / DllUnregisterServer for context menu.
//!
//! Registers under:
//!   HKCU\Software\Classes\*\shellex\ContextMenuHandlers\ownCloud
//! Default value = CLSID string.
//!
//! No elevation required — HKCU registration only.

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCreateKeyExW, RegDeleteKeyW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::core::GUID;

use crate::CLSID_OC_CONTEXT_MENU;

const HANDLER_KEY: &str =
    "Software\\Classes\\*\\shellex\\ContextMenuHandlers\\ownCloud";

fn guid_to_string(g: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1,
        g.data2,
        g.data3,
        g.data4[0],
        g.data4[1],
        g.data4[2],
        g.data4[3],
        g.data4[4],
        g.data4[5],
        g.data4[6],
        g.data4[7],
    )
}

pub fn register() -> Result<(), windows::core::Error> {
    let key_wide: Vec<u16> = HANDLER_KEY
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut hkey = HKEY::default();

    // SAFETY: `key_wide` is a valid null-terminated UTF-16 string.
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_wide.as_ptr()),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(windows::core::Error::from_win32());
    }

    let clsid_str = guid_to_string(&CLSID_OC_CONTEXT_MENU);
    let value_bytes: Vec<u8> = clsid_str
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|c| c.to_le_bytes())
        .collect();

    // SAFETY: `hkey` is a valid open registry key obtained above.
    let result2 = unsafe {
        RegSetValueExW(hkey, PCWSTR::null(), 0, REG_SZ, Some(&value_bytes))
    };

    // SAFETY: Must close the key handle to release the OS resource.
    unsafe { windows::Win32::System::Registry::RegCloseKey(hkey) };

    if result2 != ERROR_SUCCESS {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

pub fn unregister() -> Result<(), windows::core::Error> {
    let key_wide: Vec<u16> = HANDLER_KEY
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `key_wide` is a valid null-terminated wide string.
    // Ignore error — key may already be absent.
    unsafe {
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key_wide.as_ptr()));
    }
    Ok(())
}
```

- [ ] Add `PSTR` import to `oc-contextmenu/src/lib.rs`:

```rust
use windows::core::PSTR;
```

---

## Build Instructions

```powershell
# Install the MSVC target (one-time setup)
rustup target add x86_64-pc-windows-msvc

# Build all three DLLs in release mode
cd shell-integration\windows
cargo build --release --target x86_64-pc-windows-msvc
```

Output DLLs:
- `target\x86_64-pc-windows-msvc\release\oc_ipc.dll`
- `target\x86_64-pc-windows-msvc\release\oc_overlay.dll`
- `target\x86_64-pc-windows-msvc\release\oc_contextmenu.dll`

---

## Registration

Register (HKCU — no elevation required):

```powershell
# /s = silent, /n = no self-registration, /i:user = per-user (HKCU)
regsvr32 /s /n /i:user target\x86_64-pc-windows-msvc\release\oc_overlay.dll
regsvr32 /s /n /i:user target\x86_64-pc-windows-msvc\release\oc_contextmenu.dll
```

Unregister:

```powershell
regsvr32 /u /s /n /i:user target\x86_64-pc-windows-msvc\release\oc_overlay.dll
regsvr32 /u /s /n /i:user target\x86_64-pc-windows-msvc\release\oc_contextmenu.dll
```

Verify registry keys were written:

```powershell
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\ShellIconOverlayIdentifiers"
reg query "HKCU\Software\Classes\*\shellex\ContextMenuHandlers\ownCloud"
```

---

## Manual Test Checklist

- [ ] Build succeeds for `x86_64-pc-windows-msvc` target with `cargo build --release`.
- [ ] `oc_overlay.dll` exports `DllGetClassObject`, `DllCanUnloadNow`, `DllRegisterServer`, `DllUnregisterServer` (verify with `dumpbin /exports oc_overlay.dll`).
- [ ] `oc_contextmenu.dll` exports the same four symbols (verify with `dumpbin /exports oc_contextmenu.dll`).
- [ ] `regsvr32 /s /n /i:user oc_overlay.dll` exits with code 0 and no error dialog.
- [ ] `regsvr32 /s /n /i:user oc_contextmenu.dll` exits with code 0 and no error dialog.
- [ ] Registry key `HKCU\...\ShellIconOverlayIdentifiers\ownCloudOK` exists with CLSID value `{ABCD0001-1234-5678-90AB-CDEF01234567}`.
- [ ] Registry key `HKCU\...\ShellIconOverlayIdentifiers\ownCloudError` exists with value `{ABCD0004-1234-5678-90AB-CDEF01234567}`.
- [ ] Registry key `HKCU\Software\Classes\*\shellex\ContextMenuHandlers\ownCloud` exists with CLSID value `{ABCD0010-1234-5678-90AB-CDEF01234567}`.
- [ ] Start `ocsyncd` daemon and open File Explorer.
- [ ] Navigate to a folder monitored by the sync client. Overlay icons appear on files (green check for OK, spinner for SYNC, etc.).
- [ ] Right-click a file in the synced folder. An "ownCloud" submenu appears.
- [ ] The submenu contains items matching what `GET_MENU_ITEMS` returns for that file.
- [ ] Clicking "Share" (or equivalent) sends `SHARE:path` to the daemon without error.
- [ ] Clicking a disabled item has no effect (item is greyed out and InvokeCommand is not called).
- [ ] `DllCanUnloadNow` returns `S_OK` after closing all Explorer windows (verify with a COM harness or Process Monitor).
- [ ] `regsvr32 /u /s /n /i:user oc_overlay.dll` removes all five `ownCloud*` overlay keys.
- [ ] `regsvr32 /u /s /n /i:user oc_contextmenu.dll` removes the `ownCloud` context menu handler key.
- [ ] Run unit tests: `cargo test -p oc-ipc` and `cargo test -p oc-contextmenu` pass on any platform (integration tests skipped without `--ignored`).
- [ ] `menu_builder` unit tests pass: `cargo test -p oc-contextmenu menu_builder`.

//! registration.rs — DllRegisterServer / DllUnregisterServer for overlays.
//!
//! Writes (or deletes) keys under:
//!   HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\
//!       ShellIconOverlayIdentifiers\ownCloud{Name}
//!
//! Each key's default value is the CLSID string.
//! No elevation is required because we write to HKCU.

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

use crate::{
    CLSID_OC_OVERLAY_ERROR, CLSID_OC_OVERLAY_EXCLUDED, CLSID_OC_OVERLAY_OK,
    CLSID_OC_OVERLAY_SYNC, CLSID_OC_OVERLAY_WARNING,
};
use windows::core::GUID;

const BASE_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\
                        \\ShellIconOverlayIdentifiers";

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

fn write_reg_key(subkey: &str, value: &str) -> Result<(), windows::core::Error> {
    let full_path: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let mut hkey = HKEY::default();

    // SAFETY: `full_path` is a valid null-terminated UTF-16 string.
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

    let value_wide: Vec<u8> = value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|c| c.to_le_bytes())
        .collect();

    // SAFETY: `hkey` is a valid open registry key.
    let result2 =
        unsafe { RegSetValueExW(hkey, PCWSTR::null(), 0, REG_SZ, Some(&value_wide)) };

    // SAFETY: Must close to release the OS handle.
    unsafe { RegCloseKey(hkey) };

    if result2 != ERROR_SUCCESS {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

pub fn register() -> Result<(), windows::core::Error> {
    let overlays = [
        ("ownCloudOK", &CLSID_OC_OVERLAY_OK),
        ("ownCloudSync", &CLSID_OC_OVERLAY_SYNC),
        ("ownCloudWarning", &CLSID_OC_OVERLAY_WARNING),
        ("ownCloudError", &CLSID_OC_OVERLAY_ERROR),
        ("ownCloudExcluded", &CLSID_OC_OVERLAY_EXCLUDED),
    ];
    for (name, clsid) in &overlays {
        let subkey = format!("{}\\{}", BASE_KEY, name);
        write_reg_key(&subkey, &guid_to_string(clsid))?;
    }
    Ok(())
}

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
        let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `subkey_wide` is a valid null-terminated wide string.
        // Ignore errors for non-existent keys.
        unsafe {
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(subkey_wide.as_ptr()));
        }
    }
    Ok(())
}

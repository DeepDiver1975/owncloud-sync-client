//! registration.rs — DllRegisterServer / DllUnregisterServer for context menu.
//!
//! Registers under:
//!   HKCU\Software\Classes\*\shellex\ContextMenuHandlers\ownCloud
//! Default value = CLSID string. No elevation required.

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

use crate::CLSID_OC_CONTEXT_MENU;
use windows::core::GUID;

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
    let result2 =
        unsafe { RegSetValueExW(hkey, PCWSTR::null(), 0, REG_SZ, Some(&value_bytes)) };

    // SAFETY: Must close the key handle.
    unsafe { RegCloseKey(hkey) };

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
    unsafe {
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key_wide.as_ptr()));
    }
    Ok(())
}

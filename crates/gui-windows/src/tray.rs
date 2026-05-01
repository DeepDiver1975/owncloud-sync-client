#[cfg(target_os = "windows")]
pub mod inner {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };

    pub const WM_TRAY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

    pub fn add_tray_icon(hwnd: HWND) {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        let tip: Vec<u16> = "ownCloud Sync\0".encode_utf16().collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }

    pub fn remove_tray_icon(hwnd: HWND) {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

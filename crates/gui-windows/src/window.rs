#[cfg(target_os = "windows")]
pub mod inner {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::*;

    pub unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    pub fn class_name() -> Vec<u16> {
        "OwnCloudSync\0".encode_utf16().collect()
    }

    pub fn window_title() -> Vec<u16> {
        "ownCloud Sync\0".encode_utf16().collect()
    }
}

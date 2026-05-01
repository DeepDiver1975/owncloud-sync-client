#[cfg(target_os = "windows")]
mod tray;
#[cfg(target_os = "windows")]
mod window;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("gui-windows is Windows-only");
}

#[cfg(target_os = "windows")]
fn main() {
    use daemon::paths::platform_gui_socket_path;
    use gui_core::{Action, AppCore};
    use std::sync::{Arc, Mutex};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let socket = platform_gui_socket_path();
    let core = rt.block_on(AppCore::init(&socket));
    let core = Arc::new(Mutex::new(core));

    unsafe {
        let class_name = window::inner::class_name();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window::inner::wnd_proc),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let title = window::inner::window_title();
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            480,
            360,
            None,
            None,
            None,
            None,
        )
        .expect("CreateWindowExW failed");

        tray::inner::add_tray_icon(hwnd);
        ShowWindow(hwnd, SW_SHOW);

        SetTimer(hwnd, 1, 50, None);

        let mut msg = MSG::default();
        loop {
            match GetMessageW(&mut msg, None, 0, 0).0 {
                -1 | 0 => break,
                _ => {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);

                    if msg.message == WM_TIMER {
                        let mut guard = core.lock().unwrap();
                        guard.poll_events();
                        let vm = guard.view_model();
                        tracing::debug!("view: {:?}", vm.active_view);
                    }
                }
            }
        }

        tray::inner::remove_tray_icon(hwnd);
    }
}

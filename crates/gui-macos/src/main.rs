#[cfg(target_os = "macos")]
mod tray;
#[cfg(target_os = "macos")]
mod window;

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("gui-macos is macOS-only");
}

#[cfg(target_os = "macos")]
fn main() {
    use daemon::paths::platform_gui_socket_path;
    use gui_core::AppCore;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;
    use std::ptr::NonNull;
    use std::sync::{Arc, Mutex};

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
        let mtm = MainThreadMarker::new_unchecked();
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

        let window = window::inner::create_window();
        window.makeKeyAndOrderFront(None);

        let _status_item = tray::inner::create_status_item();

        // Poll daemon events every 50ms via NSTimer
        let core_timer = core.clone();
        let timer_block =
            block2::RcBlock::new(move |_timer: NonNull<objc2_foundation::NSTimer>| {
                let mut guard = core_timer.lock().unwrap();
                guard.poll_events();
                let vm = guard.view_model();
                tracing::debug!("view: {:?}", vm.active_view);
            });
        let interval = 0.05_f64;
        let _timer = objc2_foundation::NSTimer::scheduledTimerWithTimeInterval_repeats_block(
            interval,
            true,
            &*timer_block,
        );

        app.run();
    }
}

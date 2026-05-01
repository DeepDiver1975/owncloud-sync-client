#[cfg(target_os = "linux")]
mod tray;
#[cfg(target_os = "linux")]
mod window;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("gui-gtk is Linux-only");
}

#[cfg(target_os = "linux")]
fn main() {
    use daemon::paths::platform_gui_socket_path;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gui_core::{Action, AppCore};
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

    let app = libadwaita::Application::builder()
        .application_id("org.owncloud.Sync")
        .build();

    let core_clone = core.clone();
    app.connect_activate(move |app| {
        let window = crate::window::build_window(app);

        {
            let guard = core_clone.lock().unwrap();
            let vm = guard.view_model();
            crate::window::render_view_model(&window, &vm);
        }

        let core_poll = core_clone.clone();
        let window_poll = window.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            let mut guard = core_poll.lock().unwrap();
            if guard.poll_events() {
                let vm = guard.view_model();
                crate::window::render_view_model(&window_poll, &vm);
            }
            glib::ControlFlow::Continue
        });

        window.present();
    });

    let core_tray = core.clone();
    let _tray = ksni::TrayService::new(crate::tray::OwncloudTray {
        on_open: Box::new(|| tracing::info!("tray: open")),
        on_quit: Box::new(move || {
            core_tray.lock().unwrap().apply(Action::Quit);
            std::process::exit(0);
        }),
    })
    .spawn();

    let exit_code = app.run();
    std::process::exit(exit_code.into());
}

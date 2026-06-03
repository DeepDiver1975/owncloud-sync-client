use crate::app::Message;

/// Fully-resolved labels for a tray menu rebuild.
///
/// All strings are already localized (and, for `daemon_status`, already carry
/// the running/stopped marker) so the GTK thread can build the menu without
/// touching i18n state.
#[derive(Debug, Clone)]
pub struct TrayState {
    /// Whether the daemon is currently running. Drives whether the status item
    /// is disabled (running = informational) or enabled (stopped = clickable).
    pub daemon_running: bool,
    /// Label for the daemon-status item, including its marker prefix.
    pub daemon_status: String,
    pub open: String,
    pub about: String,
    pub quit: String,
}

// The tray-icon backend on Linux is driven by a GTK event loop, so the real
// implementation depends on the `gtk` crate (Linux-only). macOS/Windows have
// no gtk crate available, so they fall back to the no-op stub below.
#[cfg(all(feature = "tray-icon", target_os = "linux"))]
mod inner {
    use super::TrayState;
    use tray_icon::{
        menu::{Menu, MenuId, MenuItem, PredefinedMenuItem},
        Icon, TrayIconBuilder,
    };

    // PNG bytes produced by build.rs → $OUT_DIR/owncloud-icon-16.png
    const ICON_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/owncloud-icon-16.png"));

    fn load_icon() -> anyhow::Result<Icon> {
        let decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
        let mut reader = decoder.read_info()?;
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf)?;
        let rgba = &buf[..info.buffer_size()];
        Ok(Icon::from_rgba(rgba.to_vec(), info.width, info.height)?)
    }

    /// Handles to the mutable menu items, kept so the menu can be updated in
    /// place rather than rebuilt. Rebuilding (via `set_menu`) causes the tray
    /// menu to fully repaint and flicker; `set_text`/`set_enabled` on the live
    /// items does not.
    struct MenuItems {
        daemon_status: MenuItem,
        open: MenuItem,
        about: MenuItem,
        quit: MenuItem,
    }

    impl MenuItems {
        /// Apply a new state to the existing items in place.
        fn apply(&self, state: &TrayState) {
            self.daemon_status.set_text(&state.daemon_status);
            // Disabled (informational) when running; enabled (clickable to
            // start) when stopped.
            self.daemon_status.set_enabled(!state.daemon_running);
            self.open.set_text(&state.open);
            self.about.set_text(&state.about);
            self.quit.set_text(&state.quit);
        }
    }

    fn build_menu(state: &TrayState) -> anyhow::Result<(Menu, MenuItems)> {
        let menu = Menu::new();
        let daemon_status = MenuItem::with_id(
            MenuId::new("daemon_status"),
            &state.daemon_status,
            !state.daemon_running,
            None,
        );
        let open = MenuItem::with_id(MenuId::new("open"), &state.open, true, None);
        let about = MenuItem::with_id(MenuId::new("about"), &state.about, true, None);
        let quit = MenuItem::with_id(MenuId::new("quit"), &state.quit, true, None);

        menu.append(&daemon_status)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&open)?;
        menu.append(&about)?;
        menu.append(&quit)?;

        Ok((
            menu,
            MenuItems {
                daemon_status,
                open,
                about,
                quit,
            },
        ))
    }

    pub struct TrayHandle {
        _gtk_thread: std::thread::JoinHandle<()>,
        menu_tx: std::sync::mpsc::SyncSender<TrayState>,
    }

    impl TrayHandle {
        pub fn build() -> anyhow::Result<Self> {
            let icon = load_icon()?;

            // tray-icon on Linux requires gtk::init() and a running GTK event loop.
            // We start a dedicated thread that owns all GTK objects; iced never touches GTK.
            let (ready_tx, ready_rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();
            let (menu_tx, menu_rx) = std::sync::mpsc::sync_channel::<TrayState>(1);

            // The daemon has not connected yet at startup, so the initial menu
            // shows the stopped state. main.rs rebuilds it once connection state
            // is known. We resolve the labels here on the iced thread (where the
            // i18n locale lives) and hand the GTK thread fully-built strings.
            let initial = super::tray_state(false);

            let gtk_thread = std::thread::spawn(move || {
                if let Err(e) = gtk::init() {
                    let _ = ready_tx.send(Err(anyhow::anyhow!("gtk::init failed: {e}")));
                    return;
                }

                let build_result =
                    (|| -> anyhow::Result<(tray_icon::TrayIcon, MenuItems)> {
                        let (menu, items) = build_menu(&initial)?;
                        let icon_handle = TrayIconBuilder::new()
                            .with_icon(icon)
                            .with_menu(Box::new(menu))
                            .with_tooltip("ownCloud Sync")
                            .build()?;
                        Ok((icon_handle, items))
                    })();

                match build_result {
                    Ok((icon_handle, items)) => {
                        let _ = ready_tx.send(Ok(()));
                        // Poll for state updates while the GTK main loop runs and
                        // apply them to the existing menu items in place. The
                        // TrayIcon handle is kept alive (moved in) for the loop's
                        // lifetime; we never rebuild the menu, so no flicker.
                        let _icon_handle = icon_handle;
                        let menu_rx = std::cell::RefCell::new(menu_rx);
                        gtk::glib::timeout_add_local(
                            std::time::Duration::from_millis(100),
                            move || {
                                while let Ok(state) = menu_rx.borrow().try_recv() {
                                    items.apply(&state);
                                }
                                gtk::glib::ControlFlow::Continue
                            },
                        );
                        gtk::main();
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            });

            ready_rx
                .recv()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("GTK thread died before signalling")))?;

            Ok(Self {
                _gtk_thread: gtk_thread,
                menu_tx,
            })
        }

        pub fn rebuild_menu(&self, state: TrayState) {
            let _ = self.menu_tx.try_send(state);
        }

        pub fn tray_events(&self) -> iced::Subscription<super::Message> {
            use iced::futures::SinkExt;
            use iced::stream;
            use iced::Subscription;
            use tray_icon::menu::MenuEvent;

            // `run` identifies the subscription by the type of the stream builder
            // (a unique `fn` item here), replacing the explicit id used in 0.13.
            Subscription::run(|| {
                stream::channel(
                    8,
                    |mut tx: iced::futures::channel::mpsc::Sender<super::Message>| async move {
                        loop {
                            // MenuEvent::receiver() is a crossbeam Receiver; we can't .await it,
                            // so we poll at 50 ms intervals to stay async-friendly.
                            match MenuEvent::receiver().try_recv() {
                                Ok(event) => {
                                    let msg = if event.id == tray_icon::menu::MenuId::new("quit") {
                                        super::Message::Quit
                                    } else if event.id == tray_icon::menu::MenuId::new("about") {
                                        super::Message::ShowAbout
                                    } else if event.id
                                        == tray_icon::menu::MenuId::new("daemon_status")
                                    {
                                        // Only fires when stopped; the item is disabled (no
                                        // event) while the daemon is running.
                                        super::Message::StartDaemon
                                    } else {
                                        super::Message::ToggleWindow
                                    };
                                    let _ = tx.send(msg).await;
                                }
                                Err(_) => {
                                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                }
                            }
                        }
                    },
                )
            })
        }
    }

    impl Drop for TrayHandle {
        fn drop(&mut self) {
            // Signal the GTK event loop to exit so the thread can join cleanly.
            gtk::glib::idle_add_once(gtk::main_quit);
        }
    }
}

#[cfg(not(all(feature = "tray-icon", target_os = "linux")))]
mod inner {
    use super::TrayState;

    pub struct TrayHandle;

    impl TrayHandle {
        pub fn build() -> anyhow::Result<Self> {
            Ok(Self)
        }

        pub fn rebuild_menu(&self, _state: TrayState) {}

        pub fn tray_events(&self) -> iced::Subscription<super::Message> {
            iced::Subscription::none()
        }
    }
}

pub use inner::TrayHandle;

/// Build a [`TrayState`] for the given daemon-running flag, resolving labels
/// against the current i18n locale. Shared by the initial tray build and the
/// runtime rebuilds driven from `app`/`main`.
pub fn tray_state(daemon_running: bool) -> TrayState {
    use rust_i18n::t;
    let (marker, status_key) = if daemon_running {
        ("●", "tray_daemon_running")
    } else {
        ("○", "tray_daemon_stopped")
    };
    TrayState {
        daemon_running,
        daemon_status: format!("{marker} {}", t!(status_key)),
        open: t!("tray_open").to_string(),
        about: t!("tray_about").to_string(),
        quit: t!("tray_quit").to_string(),
    }
}

// iced requires App (and all its fields) to implement Clone + Debug.
// TrayHandle wraps a non-Clone OS resource; we never actually clone App
// at runtime — iced just requires the bound. Cloning a live tray would
// create a duplicate icon, so we panic to make any accidental clone visible.
impl Clone for TrayHandle {
    fn clone(&self) -> Self {
        panic!("TrayHandle must not be cloned")
    }
}

impl std::fmt::Debug for TrayHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrayHandle")
    }
}

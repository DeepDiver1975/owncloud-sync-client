use crate::app::Message;

#[cfg(feature = "tray-icon")]
mod inner {
    use tray_icon::{
        menu::{Menu, MenuItem},
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

    pub struct TrayHandle {
        _gtk_thread: std::thread::JoinHandle<()>,
    }

    impl TrayHandle {
        pub fn build() -> anyhow::Result<Self> {
            let icon = load_icon()?;

            // tray-icon on Linux requires gtk::init() and a running GTK event loop.
            // We start a dedicated thread that owns all GTK objects; iced never touches GTK.
            let (ready_tx, ready_rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();

            let gtk_thread = std::thread::spawn(move || {
                if let Err(e) = gtk::init() {
                    let _ = ready_tx.send(Err(anyhow::anyhow!("gtk::init failed: {e}")));
                    return;
                }

                let build_result = (|| -> anyhow::Result<tray_icon::TrayIcon> {
                    let menu = Menu::new();
                    use tray_icon::menu::MenuId;
                    let open_item = MenuItem::with_id(MenuId::new("open"), "Open", true, None);
                    let about_item = MenuItem::with_id(MenuId::new("about"), "About", true, None);
                    let quit_item = MenuItem::with_id(MenuId::new("quit"), "Quit", true, None);
                    menu.append(&open_item)?;
                    menu.append(&about_item)?;
                    menu.append(&quit_item)?;

                    Ok(TrayIconBuilder::new()
                        .with_icon(icon)
                        .with_menu(Box::new(menu))
                        .with_tooltip("ownCloud Sync")
                        .build()?)
                })();

                match build_result {
                    Ok(_icon_handle) => {
                        let _ = ready_tx.send(Ok(()));
                        // _icon_handle must stay alive; gtk::main() runs until gtk::main_quit()
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
            })
        }

        pub fn tray_events(&self) -> iced::Subscription<super::Message> {
            use iced::futures::SinkExt;
            use iced::stream;
            use iced::Subscription;
            use tray_icon::menu::MenuEvent;

            Subscription::run_with_id(
                "tray-menu-events",
                stream::channel(8, |mut tx| async move {
                    loop {
                        // MenuEvent::receiver() is a crossbeam Receiver; we can't .await it,
                        // so we poll at 50 ms intervals to stay async-friendly.
                        match MenuEvent::receiver().try_recv() {
                            Ok(event) => {
                                let msg = if event.id == tray_icon::menu::MenuId::new("quit") {
                                    super::Message::Quit
                                } else if event.id == tray_icon::menu::MenuId::new("about") {
                                    super::Message::ShowAbout
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
                }),
            )
        }
    }

    impl Drop for TrayHandle {
        fn drop(&mut self) {
            // Signal the GTK event loop to exit so the thread can join cleanly.
            gtk::glib::idle_add_once(gtk::main_quit);
        }
    }
}

#[cfg(not(feature = "tray-icon"))]
mod inner {
    pub struct TrayHandle;

    impl TrayHandle {
        pub fn build() -> anyhow::Result<Self> {
            Ok(Self)
        }

        pub fn tray_events(&self) -> iced::Subscription<super::Message> {
            iced::Subscription::none()
        }
    }
}

pub use inner::TrayHandle;

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

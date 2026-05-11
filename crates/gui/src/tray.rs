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
        pub(super) _icon: tray_icon::TrayIcon,
    }

    impl TrayHandle {
        pub fn build() -> anyhow::Result<Self> {
            let icon = load_icon()?;

            let menu = Menu::new();
            let open_item = MenuItem::new("Open", true, None);
            let quit_item = MenuItem::new("Quit", true, None);
            menu.append(&open_item)?;
            menu.append(&quit_item)?;

            let icon_handle = TrayIconBuilder::new()
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .with_tooltip("ownCloud Sync")
                .build()?;

            Ok(Self { _icon: icon_handle })
        }

        pub fn tray_events(&self) -> iced::Subscription<super::Message> {
            use iced::stream;
            use iced::futures::SinkExt;
            use iced::Subscription;
            use tray_icon::menu::MenuEvent;

            Subscription::run_with_id("tray-menu-events", stream::channel(8, |mut tx| async move {
                loop {
                    // MenuEvent::receiver() is a crossbeam Receiver; we can't .await it,
                    // so we poll at 50 ms intervals to stay async-friendly.
                    match MenuEvent::receiver().try_recv() {
                        Ok(event) => {
                            let msg = if event.id.0.contains("Quit") {
                                super::Message::Quit
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
            }))
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

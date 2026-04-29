use crate::app::Message;

#[cfg(feature = "tray-icon")]
mod inner {
    use tray_icon::{menu::Menu, menu::MenuItem, TrayIconBuilder};

    pub struct TrayHandle {
        _icon: tray_icon::TrayIcon,
    }

    impl TrayHandle {
        pub fn build() -> anyhow::Result<Self> {
            let menu = Menu::new();
            let open_item = MenuItem::new("Open", true, None);
            let quit_item = MenuItem::new("Quit", true, None);
            menu.append(&open_item)?;
            menu.append(&quit_item)?;

            let icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("ownCloud Sync")
                .build()?;

            Ok(Self { _icon: icon })
        }

        pub fn poll_message(&self) -> Option<super::Message> {
            use tray_icon::menu::MenuEvent;
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id.0.contains("Quit") {
                    return Some(super::Message::Quit);
                } else {
                    return Some(super::Message::ToggleWindow);
                }
            }
            None
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

        pub fn poll_message(&self) -> Option<super::Message> {
            None
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

#[cfg(target_os = "macos")]
pub mod inner {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
    use objc2_foundation::{MainThreadMarker, NSString};

    pub fn create_status_item() -> Retained<NSStatusItem> {
        unsafe {
            let mtm = MainThreadMarker::new_unchecked();

            let bar = NSStatusBar::systemStatusBar();
            // NSVariableStatusItemLength = -1.0
            let item = bar.statusItemWithLength(-1.0_f64);

            let title = NSString::from_str("☁");
            if let Some(button) = item.button(mtm) {
                button.setTitle(&title);
            }

            let menu = NSMenu::new(mtm);

            let open_title = NSString::from_str("Open ownCloud Sync");
            let open_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &open_title,
                None,
                &NSString::from_str(""),
            );
            menu.addItem(&open_item);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            let quit_title = NSString::from_str("Quit");
            let quit_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &quit_title,
                Some(objc2::sel!(terminate:)),
                &NSString::from_str("q"),
            );
            menu.addItem(&quit_item);

            item.setMenu(Some(&menu));
            item
        }
    }
}

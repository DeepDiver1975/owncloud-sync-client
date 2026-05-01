#[cfg(target_os = "macos")]
pub mod inner {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{CGPoint, CGRect, CGSize};

    pub fn create_window() -> Retained<NSWindow> {
        unsafe {
            let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(480.0, 360.0));
            let style = NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable;
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                &NSWindow::alloc(),
                rect,
                style,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            );
            window.setTitle(&objc2_foundation::NSString::from_str("ownCloud Sync"));
            window.center();
            window
        }
    }
}

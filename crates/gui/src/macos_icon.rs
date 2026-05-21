use objc2::ClassType;
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::{MainThreadMarker, NSData};

const PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/owncloud-icon-128.png"));

/// Sets NSApplication.applicationIconImage from the embedded PNG.
///
/// Must be called after applicationDidFinishLaunching has fired — calling it
/// earlier causes macOS to reset the icon when setActivationPolicy is called.
pub fn set_app_icon() {
    unsafe {
        let Some(mtm) = MainThreadMarker::new() else {
            tracing::warn!("set_app_icon called off main thread — skipping");
            return;
        };
        let data = NSData::dataWithBytes_length(
            PNG.as_ptr() as *mut std::ffi::c_void,
            PNG.len(),
        );
        if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
            NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
        }
    }
}

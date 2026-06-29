// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ownCloud Sync Contributors

//! macOS main menu bar and activation policy.
//!
//! Fixes two macOS-only regressions:
//!  - #84: no Dock icon while running — the app never set an activation policy
//!    (and the packaged Info.plist forced accessory mode via `LSUIElement`).
//!  - #83: no menu bar — the app never installed an `NSMenu`.
//!
//! The menu is built directly with `objc2-app-kit` (no extra crate). Menu items
//! use the *standard* first-responder selectors (`terminate:`, `cut:`, …) with a
//! `None` target, so AppKit dispatches them down the responder chain to whatever
//! view/window is key (the text fields handle clipboard actions, `NSApplication`
//! handles `terminate:`/`orderFrontStandardAboutPanel:`). This gives working
//! ⌘Q and clipboard shortcuts without any custom target objects.

use objc2::rc::Retained;
use objc2::{sel, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString};
use rust_i18n::t;

/// A single menu item in the [`MenuSpec`] descriptor.
///
/// `selector` is the Objective-C selector *name* (e.g. `"terminate:"`) so the
/// spec is plain, assertable data that needs no Objective-C runtime. The real
/// `Sel` used when building the menu is derived from the same selector-name
/// constants via [`sel_for`], so the spec and the installed menu cannot drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSpec {
    pub title: String,
    pub selector: &'static str,
    pub key: &'static str,
}

/// A top-level menu and its items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSpec {
    pub title: String,
    pub items: Vec<ItemSpec>,
}

// The single source of truth for selector names. Both `main_menu_spec` (the
// assertable descriptor) and `sel_for` (the real `Sel` resolution used by
// `build_main_menu`) reference these constants, so the spec and the installed
// menu are built from the same data and cannot drift.
const SEL_ABOUT: &str = "orderFrontStandardAboutPanel:";
const SEL_QUIT: &str = "terminate:";
const SEL_CUT: &str = "cut:";
const SEL_COPY: &str = "copy:";
const SEL_PASTE: &str = "paste:";
const SEL_SELECT_ALL: &str = "selectAll:";

/// Resolve a selector name from the spec to a real Objective-C `Sel`.
///
/// Returns `None` for an unknown name (which would indicate a programming error
/// — the spec and this table drifting apart).
fn sel_for(name: &str) -> Option<objc2::runtime::Sel> {
    Some(match name {
        SEL_ABOUT => sel!(orderFrontStandardAboutPanel:),
        SEL_QUIT => sel!(terminate:),
        SEL_CUT => sel!(cut:),
        SEL_COPY => sel!(copy:),
        SEL_PASTE => sel!(paste:),
        SEL_SELECT_ALL => sel!(selectAll:),
        _ => return None,
    })
}

/// The activation policy the app installs at launch.
///
/// `.Regular` = a normal windowed app that owns a Dock icon and menu bar (the
/// correct choice for a sync client with a real window, vs. a menu-bar-only
/// agent which would use `.Accessory`).
pub(crate) fn activation_policy() -> NSApplicationActivationPolicy {
    NSApplicationActivationPolicy::Regular
}

/// Pure, testable description of the main menu.
///
/// The first (app) menu's submenu title is the app name; macOS substitutes the
/// running process/bundle name for the actual app-menu label, so this title is
/// mostly cosmetic but kept meaningful.
pub fn main_menu_spec() -> Vec<MenuSpec> {
    vec![
        MenuSpec {
            title: "ownCloud".to_string(),
            items: vec![
                ItemSpec {
                    title: t!("menu_about").to_string(),
                    selector: SEL_ABOUT,
                    key: "",
                },
                ItemSpec {
                    title: t!("menu_quit").to_string(),
                    selector: SEL_QUIT,
                    key: "q",
                },
            ],
        },
        MenuSpec {
            title: t!("menu_edit").to_string(),
            items: vec![
                ItemSpec {
                    title: t!("menu_cut").to_string(),
                    selector: SEL_CUT,
                    key: "x",
                },
                ItemSpec {
                    title: t!("menu_copy").to_string(),
                    selector: SEL_COPY,
                    key: "c",
                },
                ItemSpec {
                    title: t!("menu_paste").to_string(),
                    selector: SEL_PASTE,
                    key: "v",
                },
                ItemSpec {
                    title: t!("menu_select_all").to_string(),
                    selector: SEL_SELECT_ALL,
                    key: "a",
                },
            ],
        },
    ]
}

/// Build a real `NSMenu` from [`main_menu_spec`].
///
/// Each top-level [`MenuSpec`] becomes an `NSMenuItem` in the menu bar whose
/// `submenu` holds the actual items. Items use `None` as their target so the
/// standard selectors dispatch down the responder chain.
pub fn build_main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let main_menu = NSMenu::new(mtm);

    for menu_spec in main_menu_spec() {
        // The bar-level item is just a container; AppKit shows its submenu.
        let bar_item = NSMenuItem::new(mtm);
        let submenu =
            NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&menu_spec.title));

        for item in &menu_spec.items {
            let Some(selector) = sel_for(item.selector) else {
                tracing::warn!(
                    "macos_menu: unknown selector {:?} — skipping item",
                    item.selector
                );
                continue;
            };
            // SAFETY: `selector` is a valid selector resolved from `sel_for`,
            // matching the documented `initWithTitle_action_keyEquivalent`
            // contract. A `None` target makes AppKit route the action through
            // the responder chain.
            let menu_item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(&item.title),
                    Some(selector),
                    &NSString::from_str(item.key),
                )
            };
            submenu.addItem(&menu_item);
        }

        bar_item.setSubmenu(Some(&submenu));
        main_menu.addItem(&bar_item);
    }

    main_menu
}

/// Install the main menu on the shared `NSApplication`.
///
/// No-op (with a warning) when called off the main thread, mirroring
/// [`crate::macos_icon::set_app_icon`].
pub fn install_main_menu() {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("install_main_menu called off main thread — skipping");
        return;
    };
    let menu = build_main_menu(mtm);
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&menu));
}

/// Set the app's activation policy to `.Regular` and bring it to the front.
///
/// `.Regular` gives the app a Dock icon and menu bar (#84). `activateIgnoringOtherApps`
/// is soft-deprecated but is the simplest way to focus the app on first launch;
/// it still compiles and works on current macOS.
pub fn set_regular_activation_policy() {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("set_regular_activation_policy called off main thread — skipping");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(activation_policy());
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_item<'a>(menu: &'a MenuSpec, selector: &str) -> &'a ItemSpec {
        menu.items
            .iter()
            .find(|i| i.selector == selector)
            .unwrap_or_else(|| panic!("no item with selector {selector:?} in {:?}", menu.title))
    }

    #[test]
    fn spec_has_app_and_edit_menus() {
        // Locale is not set in tests; titles come back as the i18n keys, so we
        // match the app menu (first) positionally and the edit menu by content.
        let spec = main_menu_spec();
        assert_eq!(spec.len(), 2, "expected App + Edit menus");
    }

    #[test]
    fn app_menu_has_about_and_quit() {
        let spec = main_menu_spec();
        let app_menu = &spec[0];

        let about = find_item(app_menu, "orderFrontStandardAboutPanel:");
        assert_eq!(about.key, "");

        let quit = find_item(app_menu, "terminate:");
        assert_eq!(quit.key, "q");
    }

    #[test]
    fn edit_menu_has_clipboard_actions() {
        let spec = main_menu_spec();
        let edit_menu = &spec[1];

        let cut = find_item(edit_menu, "cut:");
        assert_eq!(cut.key, "x");

        let copy = find_item(edit_menu, "copy:");
        assert_eq!(copy.key, "c");

        let paste = find_item(edit_menu, "paste:");
        assert_eq!(paste.key, "v");

        let select_all = find_item(edit_menu, "selectAll:");
        assert_eq!(select_all.key, "a");
    }

    #[test]
    fn every_spec_selector_resolves_to_a_real_sel() {
        // Guards against the spec and the SELECTOR_TABLE / sel_for mapping
        // drifting apart: every selector in the spec must resolve.
        for menu in main_menu_spec() {
            for item in menu.items {
                assert!(
                    sel_for(item.selector).is_some(),
                    "selector {:?} in menu has no Sel mapping",
                    item.selector
                );
            }
        }
    }

    #[test]
    fn activation_policy_is_regular() {
        assert_eq!(activation_policy(), NSApplicationActivationPolicy::Regular);
    }
}

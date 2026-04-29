//! menu_builder.rs — Parse the GET_MENU_ITEMS daemon response.
//!
//! Wire format:
//!   GET_MENU_ITEMS:path\x1eitem_name:item_cmd:item_state\x1e...\n
//!
//! Fields within each item are colon-separated; records are separated by
//! the ASCII unit separator `\x1e` (0x1E).

/// A single menu item definition parsed from the daemon response.
#[derive(Debug, Clone)]
pub struct MenuItemDef {
    /// Sequential ID used to match InvokeCommand's idCmd offset.
    pub id: u32,
    /// Human-readable label shown in the submenu.
    pub label: String,
    /// Wire command sent to the daemon on invocation (e.g. "SHARE").
    pub command: String,
    /// Whether the item should be clickable (false → shown greyed out).
    pub enabled: bool,
}

/// Parse a raw GET_MENU_ITEMS response into a list of `MenuItemDef`s.
///
/// Returns an empty `Vec` on any parse error so the caller degrades
/// gracefully.
pub fn parse_menu_items(response: &str) -> Vec<MenuItemDef> {
    let mut parts = response.splitn(2, '\x1e');
    let _header = parts.next();
    let rest = match parts.next() {
        Some(r) => r,
        None => return Vec::new(),
    };

    rest.split('\x1e')
        .filter(|s| !s.is_empty())
        .enumerate()
        .filter_map(|(i, record)| {
            let mut fields = record.splitn(3, ':');
            let label = fields.next()?.to_owned();
            let command = fields.next()?.to_owned();
            let state = fields.next().unwrap_or("enabled");
            let enabled = state != "disabled";
            Some(MenuItemDef {
                id: i as u32,
                label,
                command,
                enabled,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_response() {
        assert!(parse_menu_items("").is_empty());
    }

    #[test]
    fn test_parse_single_item() {
        let response = "GET_MENU_ITEMS:C:\\foo\x1eShare:SHARE:enabled\x1e";
        let items = parse_menu_items(response);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Share");
        assert_eq!(items[0].command, "SHARE");
        assert!(items[0].enabled);
        assert_eq!(items[0].id, 0);
    }

    #[test]
    fn test_parse_multiple_items_with_disabled() {
        let response = concat!(
            "GET_MENU_ITEMS:C:\\foo\x1e",
            "Share:SHARE:enabled\x1e",
            "Copy private link:COPY_PRIVATE_LINK:enabled\x1e",
            "Make available locally:MAKE_AVAILABLE_LOCALLY:disabled\x1e",
        );
        let items = parse_menu_items(response);
        assert_eq!(items.len(), 3);
        assert!(items[0].enabled);
        assert!(items[1].enabled);
        assert!(!items[2].enabled);
        assert_eq!(items[2].command, "MAKE_AVAILABLE_LOCALLY");
    }

    #[test]
    fn test_parse_malformed_record_skipped() {
        let response = "GET_MENU_ITEMS:C:\\foo\x1eSHARE\x1eShare:SHARE:enabled\x1e";
        let items = parse_menu_items(response);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Share");
    }
}

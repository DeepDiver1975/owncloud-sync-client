use crate::protocol::FIELD_SEP;
use crate::status_resolver::StatusResolver;

pub fn handle_get_strings() -> String {
    let parts: Vec<(&str, &str)> = vec![
        ("SHARE_MENU_TITLE", "Share"),
        ("COPY_LINK", "Copy link"),
        ("MAKE_AVAILABLE", "Make available locally"),
        ("MAKE_ONLINE_ONLY", "Make online only"),
        ("OPEN_PRIVATE_LINK", "Open in browser"),
    ];

    let mut out = String::from("GET_STRINGS:");
    let pairs: Vec<String> = parts.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    out.push_str(&pairs.join(&FIELD_SEP.to_string()));
    out.push('\n');
    out
}

pub fn handle_get_menu_items(path: &str, resolver: &StatusResolver) -> String {
    if resolver.find_folder_for_path(path).is_none() {
        return format!("GET_MENU_ITEMS:{path}\n");
    }

    let mut out = format!("GET_MENU_ITEMS:{path}");
    let items = [
        "Share:SHARE:enabled",
        "Copy link:COPY_PRIVATE_LINK:enabled",
        "Make available locally:MAKE_AVAILABLE_LOCALLY:enabled",
        "Make online only:MAKE_ONLINE_ONLY:enabled",
    ];
    for item in &items {
        out.push(FIELD_SEP);
        out.push_str(item);
    }
    out.push('\n');
    out
}

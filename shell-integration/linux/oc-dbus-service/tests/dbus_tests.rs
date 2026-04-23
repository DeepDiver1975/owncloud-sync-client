use oc_dbus_service::socket_client::{parse_menu_items_line, parse_status_line};

#[test]
fn parse_status_line_ok() {
    let result = parse_status_line("STATUS:OK:/home/user/file.txt\n");
    assert_eq!(result, Some(("OK".into(), "/home/user/file.txt".into())));
}

#[test]
fn parse_status_line_with_colon_in_path() {
    let result = parse_status_line("STATUS:SYNC:/home/user/my:file.txt");
    assert_eq!(
        result,
        Some(("SYNC".into(), "/home/user/my:file.txt".into()))
    );
}

#[test]
fn parse_status_line_unknown_tag() {
    let result = parse_status_line("STATUS:NONE:/some/path");
    assert_eq!(result, Some(("NONE".into(), "/some/path".into())));
}

#[test]
fn parse_status_line_invalid_returns_none() {
    assert_eq!(parse_status_line("GARBAGE"), None);
    assert_eq!(parse_status_line(""), None);
    assert_eq!(parse_status_line("STATUS:"), None);
}

#[test]
fn parse_menu_items_two_entries() {
    let line = "GET_MENU_ITEMS:/foo\x1eShare:SHARE:enabled\x1eMake Available:MAKE_AVAILABLE_LOCALLY:disabled\n";
    let items = parse_menu_items_line(line);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], ("Share".into(), "SHARE".into(), true));
    assert_eq!(
        items[1],
        (
            "Make Available".into(),
            "MAKE_AVAILABLE_LOCALLY".into(),
            false
        )
    );
}

#[test]
fn parse_menu_items_empty_returns_empty() {
    let items = parse_menu_items_line("GET_MENU_ITEMS:/foo\n");
    assert!(items.is_empty());
}

#[test]
fn parse_menu_items_single_entry() {
    let line = "GET_MENU_ITEMS:/bar\x1eShare:SHARE:enabled\n";
    let items = parse_menu_items_line(line);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].2, true);
}

#[tokio::test]
async fn get_file_status_returns_none_when_no_daemon() {
    use oc_dbus_service::socket_client::SocketClient;
    let result = SocketClient::connect_path("/tmp/nonexistent_ocsync_test_xyz.sock").await;
    assert!(result.is_err());
}

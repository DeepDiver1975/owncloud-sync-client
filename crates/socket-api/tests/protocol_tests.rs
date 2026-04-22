use socket_api::protocol::{parse_command, format_response, Command};
use socket_api::error::SocketApiError;

#[test]
fn parse_version() {
    match parse_command("VERSION").unwrap() {
        Command::Version => {}
        other => panic!("expected Version, got {other:?}"),
    }
}

#[test]
fn parse_get_strings() {
    match parse_command("GET_STRINGS").unwrap() {
        Command::GetStrings => {}
        other => panic!("expected GetStrings, got {other:?}"),
    }
}

#[test]
fn parse_get_menu_items() {
    match parse_command("GET_MENU_ITEMS:/sync/root/file.txt").unwrap() {
        Command::GetMenuItems { path } => assert_eq!(path, "/sync/root/file.txt"),
        other => panic!("expected GetMenuItems, got {other:?}"),
    }
}

#[test]
fn parse_retrieve_file_status() {
    match parse_command("RETRIEVE_FILE_STATUS:/home/user/docs/a.pdf").unwrap() {
        Command::RetrieveFileStatus { path } => {
            assert_eq!(path, "/home/user/docs/a.pdf")
        }
        other => panic!("expected RetrieveFileStatus, got {other:?}"),
    }
}

#[test]
fn parse_retrieve_folder_status() {
    match parse_command("RETRIEVE_FOLDER_STATUS:/home/user/docs").unwrap() {
        Command::RetrieveFolderStatus { path } => {
            assert_eq!(path, "/home/user/docs")
        }
        other => panic!("expected RetrieveFolderStatus, got {other:?}"),
    }
}

#[test]
fn parse_share() {
    match parse_command("SHARE:/tmp/foo.txt").unwrap() {
        Command::Share { path } => assert_eq!(path, "/tmp/foo.txt"),
        other => panic!("expected Share, got {other:?}"),
    }
}

#[test]
fn parse_make_available_locally_single() {
    match parse_command("MAKE_AVAILABLE_LOCALLY:/a/b.txt").unwrap() {
        Command::MakeAvailableLocally { paths } => {
            assert_eq!(paths, vec!["/a/b.txt"]);
        }
        other => panic!("expected MakeAvailableLocally, got {other:?}"),
    }
}

#[test]
fn parse_make_available_locally_multiple() {
    let line = "MAKE_AVAILABLE_LOCALLY:/a/b.txt\x1e/c/d.txt\x1e/e/f.txt";
    match parse_command(line).unwrap() {
        Command::MakeAvailableLocally { paths } => {
            assert_eq!(paths, vec!["/a/b.txt", "/c/d.txt", "/e/f.txt"]);
        }
        other => panic!("expected MakeAvailableLocally, got {other:?}"),
    }
}

#[test]
fn parse_make_online_only_multiple() {
    let line = "MAKE_ONLINE_ONLY:/x/y.txt\x1e/z.txt";
    match parse_command(line).unwrap() {
        Command::MakeOnlineOnly { paths } => {
            assert_eq!(paths, vec!["/x/y.txt", "/z.txt"]);
        }
        other => panic!("expected MakeOnlineOnly, got {other:?}"),
    }
}

#[test]
fn parse_copy_private_link() {
    match parse_command("COPY_PRIVATE_LINK:/share/me.txt").unwrap() {
        Command::CopyPrivateLink { path } => {
            assert_eq!(path, "/share/me.txt")
        }
        other => panic!("expected CopyPrivateLink, got {other:?}"),
    }
}

#[test]
fn parse_v2_command() {
    match parse_command("V2/GET_CLIENT_ICON").unwrap() {
        Command::V2 { name, body } => {
            assert_eq!(name, "GET_CLIENT_ICON");
            assert_eq!(body, "");
        }
        other => panic!("expected V2, got {other:?}"),
    }
}

#[test]
fn parse_empty_line_is_error() {
    let result = parse_command("");
    assert!(
        matches!(result, Err(SocketApiError::Protocol(_))),
        "empty line should be a protocol error"
    );
}

#[test]
fn parse_unknown_command_is_error() {
    let result = parse_command("TOTALLY_UNKNOWN_CMD:arg");
    assert!(
        matches!(result, Err(SocketApiError::Protocol(_))),
        "unknown command should be a protocol error"
    );
}

#[test]
fn parse_get_menu_items_missing_path_is_error() {
    let result = parse_command("GET_MENU_ITEMS:");
    assert!(
        matches!(result, Err(SocketApiError::Protocol(_))),
        "GET_MENU_ITEMS with empty path should fail"
    );
}

#[test]
fn format_response_single_part() {
    let resp = format_response("VERSION", &["1.1"]);
    assert_eq!(resp, "VERSION:1.1\n");
}

#[test]
fn format_response_multiple_parts_uses_field_sep() {
    let resp = format_response("STATUS", &["OK", "/home/user/file.txt"]);
    assert_eq!(resp, "STATUS:OK:/home/user/file.txt\n");
}

#[test]
fn format_response_no_parts() {
    let resp = format_response("PING", &[]);
    assert_eq!(resp, "PING\n");
}

#[test]
fn format_response_get_strings_uses_field_sep() {
    let resp = format_response(
        "GET_STRINGS",
        &["SHARE_MENU_TITLE", "Share", "OPEN_PRIVATE_LINK", "Open in browser"],
    );
    assert_eq!(
        resp,
        "GET_STRINGS:SHARE_MENU_TITLE:Share:OPEN_PRIVATE_LINK:Open in browser\n"
    );
}

#[test]
fn format_response_ends_with_newline() {
    let resp = format_response("CMD", &["a", "b"]);
    assert!(resp.ends_with('\n'), "response must end with newline");
}

use socket_api::commands::share::{handle_share, handle_copy_private_link};
use socket_api::commands::v2::handle_v2_get_client_icon;

#[test]
fn share_returns_ok_response() {
    let resp = handle_share("/sync/root/doc.pdf");
    assert_eq!(resp, "SHARE:OK:/sync/root/doc.pdf\n");
}

#[test]
fn share_response_format() {
    let resp = handle_share("/any/path/here");
    assert!(resp.starts_with("SHARE:OK:"), "should start with SHARE:OK:");
    assert!(resp.ends_with('\n'), "should end with newline");
}

#[test]
fn copy_private_link_returns_ok_response() {
    let resp = handle_copy_private_link("/sync/root/image.png");
    assert_eq!(resp, "COPY_PRIVATE_LINK:OK:/sync/root/image.png\n");
}

#[test]
fn copy_private_link_response_format() {
    let resp = handle_copy_private_link("/any/path");
    assert!(resp.starts_with("COPY_PRIVATE_LINK:OK:"), "should start with COPY_PRIVATE_LINK:OK:");
    assert!(resp.ends_with('\n'));
}

#[test]
fn v2_get_client_icon_parses_id_and_returns_result() {
    let body = r#"{"id":"42","arguments":{}}"#;
    let resp = handle_v2_get_client_icon(body);
    assert!(resp.contains(r#""id":"42""#), "response should echo the request id");
    assert!(resp.contains(r#""icon":"#), "response should contain icon field");
}

#[test]
fn v2_get_client_icon_starts_with_v2_prefix() {
    let body = r#"{"id":"1","arguments":{}}"#;
    let resp = handle_v2_get_client_icon(body);
    assert!(
        resp.starts_with("V2/GET_CLIENT_ICON\n"),
        "V2 response should start with V2/GET_CLIENT_ICON\\n"
    );
}

#[test]
fn v2_get_client_icon_ends_with_newline() {
    let body = r#"{"id":"7","arguments":{}}"#;
    let resp = handle_v2_get_client_icon(body);
    assert!(resp.ends_with('\n'), "V2 response must end with newline");
}

#[test]
fn v2_get_client_icon_malformed_body_uses_unknown_id() {
    let resp = handle_v2_get_client_icon("not json at all");
    assert!(
        resp.contains(r#""id":"unknown""#),
        "malformed body should produce id=unknown, got: {resp}"
    );
}

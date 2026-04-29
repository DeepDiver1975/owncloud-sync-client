use socket_api::error::SocketApiError;

#[test]
fn all_variants_exist() {
    let _: SocketApiError =
        SocketApiError::Io(std::io::Error::other("io error"));
    let _: SocketApiError = SocketApiError::Transport("bad transport".into());
    let _: SocketApiError = SocketApiError::Protocol("bad protocol".into());
    let _: SocketApiError = SocketApiError::Vfs(vfs_core::VfsError::NotSupported);
}

#[test]
fn socket_api_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SocketApiError>();
}

#[test]
fn error_display_is_informative() {
    let e = SocketApiError::Protocol("unexpected EOF".into());
    assert!(e.to_string().contains("unexpected EOF"));
}

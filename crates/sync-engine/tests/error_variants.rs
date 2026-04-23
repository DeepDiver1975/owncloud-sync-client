use sync_engine::error::SyncError;

#[test]
fn all_variants_exist() {
    let _: SyncError = SyncError::Http {
        status: 404,
        message: "not found".into(),
    };
    let _: SyncError = SyncError::Io(std::io::Error::new(std::io::ErrorKind::Other, "test"));
    let _: SyncError = SyncError::Db("db error".into());
    let _: SyncError = SyncError::Vfs("vfs error".into());
    let _: SyncError = SyncError::Parse("parse error".into());
    let _: SyncError = SyncError::Conflict {
        path: camino::Utf8PathBuf::from("/a/b"),
    };
    let _: SyncError = SyncError::Cancelled;
}

#[test]
fn sync_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SyncError>();
}

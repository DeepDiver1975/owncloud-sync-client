use camino::Utf8PathBuf;
use std::time::SystemTime;
use sync_engine::types::*;

#[test]
fn local_entry_fields() {
    let e = LocalEntry {
        path: Utf8PathBuf::from("/tmp/a.txt"),
        mtime: SystemTime::UNIX_EPOCH,
        size: 100,
        inode: 42,
        is_virtual: false,
    };
    assert_eq!(e.size, 100);
    assert!(!e.is_virtual);
}

#[test]
fn remote_entry_fields() {
    let e = RemoteEntry {
        path: Utf8PathBuf::from("/remote/a.txt"),
        etag: "abc123".into(),
        mtime: SystemTime::UNIX_EPOCH,
        size: 200,
        file_id: "file-uuid".into(),
        permissions: 0o644,
    };
    assert_eq!(e.etag, "abc123");
}

#[test]
fn sync_instruction_variants() {
    let _up = SyncInstruction::Upload;
    let _dn = SyncInstruction::Download;
    let _dl = SyncInstruction::DeleteLocal;
    let _dr = SyncInstruction::DeleteRemote;
    let _rl = SyncInstruction::RenameLocal {
        to: Utf8PathBuf::from("/b"),
    };
    let _rr = SyncInstruction::RenameRemote {
        to: Utf8PathBuf::from("/c"),
    };
    let _co = SyncInstruction::Conflict;
    let _um = SyncInstruction::UpdateMetadata;
    let _ig = SyncInstruction::Ignore;
}

#[test]
fn sync_file_item_roundtrip() {
    let item = SyncFileItem {
        path: Utf8PathBuf::from("/a/b.txt"),
        instruction: SyncInstruction::Upload,
        direction: Direction::Up,
        etag: Some("etag1".into()),
        size: 512,
        mtime: SystemTime::UNIX_EPOCH,
        file_id: Some("fid".into()),
        checksum: None,
        error: None,
    };
    assert_eq!(item.direction, Direction::Up);
}

#[test]
fn conflict_strategy_variants() {
    let _ = ConflictStrategy::KeepBoth;
    let _ = ConflictStrategy::KeepRemote;
    let _ = ConflictStrategy::KeepLocal;
}

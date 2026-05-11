//! Exhaustive unit tests for the reconcile() pure function.

use camino::Utf8PathBuf;
use std::time::{Duration, SystemTime};
use sync_engine::reconcile::reconcile;
use sync_engine::types::*;

fn path(s: &str) -> Utf8PathBuf {
    Utf8PathBuf::from(s)
}
fn t(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn local(size: u64, mtime: SystemTime) -> LocalEntry {
    LocalEntry {
        path: path("/a.txt"),
        mtime,
        size,
        inode: 1,
        is_virtual: false,
        is_dir: false,
    }
}

fn remote(size: u64, etag: &str, mtime: SystemTime) -> RemoteEntry {
    RemoteEntry {
        path: path("/a.txt"),
        etag: etag.into(),
        mtime,
        size,
        file_id: "fid".into(),
        permissions: 0,
        is_dir: false,
    }
}

/// JournalEntry: (etag_at_last_sync, size_at_last_sync)
fn journal(etag: &str, size: u64) -> (String, u64) {
    (etag.to_string(), size)
}

#[test]
fn no_local_no_remote_no_journal() {
    let instr = reconcile(None, None, None, ConflictStrategy::KeepBoth);
    assert_eq!(instr, SyncInstruction::Ignore);
}

#[test]
fn local_only_no_journal() {
    let instr = reconcile(
        Some(local(10, t(1))),
        None,
        None,
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Upload);
}

#[test]
fn remote_only_no_journal() {
    let instr = reconcile(
        None,
        Some(remote(10, "e1", t(1))),
        None,
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Download);
}

#[test]
fn local_only_journal_present() {
    let instr = reconcile(
        Some(local(10, t(1))),
        None,
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::DeleteLocal);
}

#[test]
fn remote_only_journal_present() {
    let instr = reconcile(
        None,
        Some(remote(10, "e1", t(1))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::DeleteRemote);
}

#[test]
fn both_present_in_sync() {
    let instr = reconcile(
        Some(local(10, t(1))),
        Some(remote(10, "e1", t(1))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Ignore);
}

#[test]
fn both_present_remote_changed() {
    let instr = reconcile(
        Some(local(10, t(1))),
        Some(remote(20, "e2", t(2))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Download);
}

#[test]
fn both_present_local_changed() {
    let instr = reconcile(
        Some(local(99, t(5))),
        Some(remote(10, "e1", t(1))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Upload);
}

#[test]
fn both_changed_conflict_keepboth() {
    let instr = reconcile(
        Some(local(99, t(5))),
        Some(remote(20, "e2", t(2))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Conflict);
}

#[test]
fn both_changed_conflict_keepremote() {
    let instr = reconcile(
        Some(local(99, t(5))),
        Some(remote(20, "e2", t(2))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepRemote,
    );
    assert_eq!(instr, SyncInstruction::Download);
}

#[test]
fn both_changed_conflict_keeplocal() {
    let instr = reconcile(
        Some(local(99, t(5))),
        Some(remote(20, "e2", t(2))),
        Some(journal("e1", 10)),
        ConflictStrategy::KeepLocal,
    );
    assert_eq!(instr, SyncInstruction::Upload);
}

#[test]
fn both_present_no_journal_conflict() {
    let instr = reconcile(
        Some(local(10, t(1))),
        Some(remote(10, "e1", t(1))),
        None,
        ConflictStrategy::KeepBoth,
    );
    assert_eq!(instr, SyncInstruction::Conflict);
}

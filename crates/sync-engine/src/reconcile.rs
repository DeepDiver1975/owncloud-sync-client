use crate::types::{
    ConflictStrategy, LocalEntry, RemoteEntry, SyncInstruction,
};

/// A minimal journal baseline: the etag and size recorded after the last
/// successful sync of this path.
pub type JournalBaseline = (String, u64); // (etag, size)

/// Decide what to do with one path given optional local/remote/journal entries.
pub fn reconcile(
    local: Option<LocalEntry>,
    remote: Option<RemoteEntry>,
    journal: Option<JournalBaseline>,
    strategy: ConflictStrategy,
) -> SyncInstruction {
    match (local, remote, journal) {
        (None, None, _) => SyncInstruction::Ignore,

        (Some(_), None, None) => SyncInstruction::Upload,
        (Some(_), None, Some(_)) => SyncInstruction::DeleteLocal,

        (None, Some(_), None) => SyncInstruction::Download,
        (None, Some(_), Some(_)) => SyncInstruction::DeleteRemote,

        (Some(_), Some(_), None) => SyncInstruction::Conflict,

        (Some(loc), Some(rem), Some((j_etag, j_size))) => {
            let remote_changed = rem.etag != j_etag;
            let local_changed = loc.size != j_size;

            match (local_changed, remote_changed) {
                (false, false) => SyncInstruction::Ignore,
                (true, false)  => SyncInstruction::Upload,
                (false, true)  => SyncInstruction::Download,
                (true, true)   => match strategy {
                    ConflictStrategy::KeepBoth   => SyncInstruction::Conflict,
                    ConflictStrategy::KeepRemote => SyncInstruction::Download,
                    ConflictStrategy::KeepLocal  => SyncInstruction::Upload,
                },
            }
        }
    }
}

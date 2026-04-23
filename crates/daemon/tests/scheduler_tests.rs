use daemon::scheduler::SyncScheduler;
use uuid::Uuid;

#[test]
fn concurrent_folders_independent() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let mut s = SyncScheduler::new(vec![id1, id2]);

    s.request_sync(id1);
    s.request_sync(id2);

    let ready = s.ready_to_run();
    assert!(ready.contains(&id1));
    assert!(ready.contains(&id2));

    s.start_sync(id1);
    let ready = s.ready_to_run();
    assert!(!ready.contains(&id1));
    assert!(ready.contains(&id2));
}

#[test]
fn finish_updates_last_sync_time() {
    let id = Uuid::new_v4();
    let mut s = SyncScheduler::new(vec![id]);
    s.request_sync(id);
    s.start_sync(id);
    s.finish_sync(id);
    let state = s.state(id).unwrap();
    assert!(!state.running);
    assert!(state.last_sync.is_some());
}

#[test]
fn unknown_folder_id_is_silently_ignored() {
    let mut s = SyncScheduler::new(vec![]);
    let ghost = Uuid::new_v4();
    s.request_sync(ghost);
    assert!(s.ready_to_run().is_empty());
}

use std::collections::HashMap;
use std::time::SystemTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct FolderScheduleState {
    pub paused:    bool,
    pub running:   bool,
    pub pending:   bool,
    pub last_sync: Option<SystemTime>,
}

impl Default for FolderScheduleState {
    fn default() -> Self {
        Self { paused: false, running: false, pending: false, last_sync: None }
    }
}

pub struct SyncScheduler {
    folders: HashMap<Uuid, FolderScheduleState>,
}

impl SyncScheduler {
    pub fn new(folder_ids: Vec<Uuid>) -> Self {
        let folders = folder_ids
            .into_iter()
            .map(|id| (id, FolderScheduleState::default()))
            .collect();
        Self { folders }
    }

    pub fn add_folder(&mut self, id: Uuid) {
        self.folders.entry(id).or_default();
    }

    pub fn request_sync(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            if !state.running && !state.paused {
                state.pending = true;
            }
        }
    }

    pub fn force_request_sync(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            if !state.paused {
                state.pending = true;
            }
        }
    }

    pub fn start_sync(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            state.running = true;
            state.pending = false;
        }
    }

    pub fn finish_sync(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            state.running  = false;
            state.last_sync = Some(SystemTime::now());
        }
    }

    pub fn pause(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            state.paused = true;
        }
    }

    pub fn resume(&mut self, folder_id: Uuid) {
        if let Some(state) = self.folders.get_mut(&folder_id) {
            state.paused = false;
        }
    }

    pub fn ready_to_run(&self) -> Vec<Uuid> {
        self.folders
            .iter()
            .filter(|(_, s)| s.pending && !s.running && !s.paused)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn state(&self, folder_id: Uuid) -> Option<&FolderScheduleState> {
        self.folders.get(&folder_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_then_ready_to_run() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.request_sync(id);
        assert!(s.ready_to_run().contains(&id));
    }

    #[test]
    fn start_removes_from_ready() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.request_sync(id);
        s.start_sync(id);
        assert!(!s.ready_to_run().contains(&id));
    }

    #[test]
    fn finish_then_request_again() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.request_sync(id);
        s.start_sync(id);
        s.finish_sync(id);
        assert!(s.state(id).unwrap().last_sync.is_some());
        s.request_sync(id);
        assert!(s.ready_to_run().contains(&id));
    }

    #[test]
    fn paused_never_ready() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.pause(id);
        s.request_sync(id);
        assert!(!s.ready_to_run().contains(&id));
    }

    #[test]
    fn resume_makes_pending_ready() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.pause(id);
        s.request_sync(id);
        assert!(!s.ready_to_run().contains(&id));
        s.resume(id);
        s.request_sync(id);
        assert!(s.ready_to_run().contains(&id));
    }

    #[test]
    fn running_folder_cannot_be_double_started() {
        let id = Uuid::new_v4();
        let mut s = SyncScheduler::new(vec![id]);
        s.request_sync(id);
        s.start_sync(id);
        s.request_sync(id);
        assert!(!s.ready_to_run().contains(&id));
    }
}

use std::path::Path;
use tokio::sync::mpsc;
use uuid::Uuid;

use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};

use crate::action::{Action, BackendCommand};
use crate::daemon_conn::DaemonConnection;
use crate::model::{AccountView, FolderStatus, FolderView};
use crate::spawn::ensure_daemon_running;
use crate::view_model::{ViewKind, ViewModel};

pub struct AppCore {
    pub(crate) state: AppState,
    daemon: DaemonConnection,
    event_rx: Option<mpsc::Receiver<DaemonEvent>>,
}

impl std::fmt::Debug for AppCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppCore")
            .field("daemon_connected", &self.state.daemon_connected)
            .field("active_view", &self.state.active_view)
            .finish_non_exhaustive()
    }
}

pub(crate) struct AppState {
    pub accounts: Vec<AccountView>,
    pub active_view: ViewKind,
    pub window_visible: bool,
    pub daemon_connected: bool,
}

impl AppCore {
    pub fn new() -> Self {
        Self {
            state: AppState {
                accounts: vec![],
                active_view: ViewKind::SyncStatus,
                window_visible: true,
                daemon_connected: false,
            },
            daemon: DaemonConnection::disconnected(),
            event_rx: None,
        }
    }

    pub async fn init(socket_path: &Path) -> Self {
        let mut core = Self::new();
        if let Err(e) = ensure_daemon_running(socket_path).await {
            tracing::warn!("daemon not available: {e}");
            return core;
        }
        match DaemonConnection::connect(socket_path).await {
            Ok((conn, rx)) => {
                core.set_connection(conn, rx);
            }
            Err(e) => tracing::error!("failed to connect to daemon: {e}"),
        }
        core
    }

    pub fn set_connection(&mut self, conn: DaemonConnection, rx: mpsc::Receiver<DaemonEvent>) {
        self.daemon = conn;
        self.event_rx = Some(rx);
        self.state.daemon_connected = true;
    }

    pub fn apply(&mut self, action: Action) -> Vec<BackendCommand> {
        match action {
            Action::NavigateTo(view) => {
                self.state.active_view = view;
            }
            Action::ToggleWindow => {
                self.state.window_visible = !self.state.window_visible;
            }
            Action::AddAccountUrlChanged(url) => {
                if let ViewKind::AddAccount { url_input, .. } = &mut self.state.active_view {
                    *url_input = url;
                }
            }
            Action::AddAccountSubmit => {
                if let ViewKind::AddAccount { url_input, error } = &mut self.state.active_view {
                    let url = url_input.clone();
                    if url.is_empty() {
                        *error = Some("Please enter a server URL".to_string());
                    } else if self
                        .daemon
                        .send(DaemonCommand::AddAccount { url: url.clone() })
                    {
                        self.state.active_view = ViewKind::AddAccountWaiting {
                            account_id: Uuid::nil(),
                            url_input: url,
                        };
                    } else {
                        *error = Some("Not connected to sync daemon".to_string());
                    }
                }
            }
            Action::PauseFolder(folder_id) => {
                if self.daemon.send(DaemonCommand::PauseFolder { folder_id }) {
                    set_folder_status(&mut self.state.accounts, folder_id, FolderStatus::Paused);
                }
            }
            Action::ResumeFolder(folder_id) => {
                if self.daemon.send(DaemonCommand::ResumeFolder { folder_id }) {
                    set_folder_status(&mut self.state.accounts, folder_id, FolderStatus::Idle);
                }
            }
            Action::ForceSyncFolder(folder_id) => {
                self.daemon.send(DaemonCommand::TriggerSync { folder_id });
                // no state mutation needed — sync status comes back via events
            }
            Action::RemoveAccount(account_id) => {
                if self
                    .daemon
                    .send(DaemonCommand::RemoveAccount { account_id })
                {
                    self.state.accounts.retain(|a| a.id != account_id);
                    self.state.active_view = ViewKind::SyncStatus;
                }
            }
            Action::OpenFolder(path) => {
                return vec![BackendCommand::OpenFolder(path)];
            }
            Action::Quit => {
                self.daemon.send(DaemonCommand::Quit);
                return vec![BackendCommand::Quit];
            }
        }
        vec![]
    }

    pub fn poll_events(&mut self) -> bool {
        let Some(rx) = &mut self.event_rx else {
            return false;
        };
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok(event) => changed |= handle_event(&mut self.state, event),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    self.state.daemon_connected = false;
                    self.event_rx = None;
                    changed = true;
                    break;
                }
            }
        }
        changed
    }

    pub fn view_model(&self) -> ViewModel {
        ViewModel {
            accounts: self.state.accounts.clone(),
            active_view: self.state.active_view.clone(),
            window_visible: self.state.window_visible,
            daemon_connected: self.state.daemon_connected,
        }
    }
}

fn handle_event(state: &mut AppState, event: DaemonEvent) -> bool {
    match event {
        DaemonEvent::Ready => false,

        DaemonEvent::SyncStarted { folder_id } => {
            if let Some(f) = find_folder_mut(&mut state.accounts, folder_id) {
                f.status = FolderStatus::Syncing;
                f.errors.clear();
                true
            } else {
                false
            }
        }

        DaemonEvent::SyncProgress {
            folder_id,
            done,
            total,
        } => {
            if let Some(f) = find_folder_mut(&mut state.accounts, folder_id) {
                f.progress = Some((done, total));
                true
            } else {
                false
            }
        }

        DaemonEvent::SyncFinished { folder_id, errors } => {
            if let Some(f) = find_folder_mut(&mut state.accounts, folder_id) {
                f.progress = None;
                if errors.is_empty() {
                    f.status = FolderStatus::Idle;
                    f.errors.clear();
                } else {
                    f.status = FolderStatus::Error;
                    f.errors = errors;
                }
                true
            } else {
                false
            }
        }

        DaemonEvent::FileStatusChanged { path, status } => {
            tracing::debug!("file status changed: {path} → {status}");
            false
        }

        DaemonEvent::AccountStateChanged {
            account_id,
            state: s,
        } => {
            if s == "added" {
                if let ViewKind::AddAccountWaiting {
                    account_id: waiting_id,
                    ..
                } = &state.active_view
                {
                    if waiting_id.is_nil() || *waiting_id == account_id {
                        state.active_view = ViewKind::SyncStatus;
                        return true;
                    }
                }
            }
            tracing::debug!("account state changed: {account_id} → {s}");
            false
        }

        DaemonEvent::AccountAddStarted { account_id } => {
            if let ViewKind::AddAccountWaiting {
                account_id: stored, ..
            } = &mut state.active_view
            {
                *stored = account_id;
                true
            } else {
                false
            }
        }

        DaemonEvent::AccountAddFailed {
            account_id: _,
            reason,
        } => {
            if let ViewKind::AddAccountWaiting { url_input, .. } = &state.active_view {
                let url = url_input.clone();
                state.active_view = ViewKind::AddAccount {
                    url_input: url,
                    error: Some(reason),
                };
                true
            } else {
                tracing::warn!("AccountAddFailed received but not in AddAccountWaiting view");
                false
            }
        }
    }
}

fn find_folder_mut(accounts: &mut [AccountView], folder_id: Uuid) -> Option<&mut FolderView> {
    accounts
        .iter_mut()
        .flat_map(|a| a.folders.iter_mut())
        .find(|f| f.id == folder_id)
}

fn set_folder_status(accounts: &mut [AccountView], folder_id: Uuid, status: FolderStatus) {
    if let Some(f) = find_folder_mut(accounts, folder_id) {
        f.status = status;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daemon::gui_ipc::protocol::DaemonEvent;
    use uuid::Uuid;

    fn core_with_test_conn() -> (AppCore, tokio::sync::mpsc::Sender<DaemonEvent>) {
        let mut core = AppCore::new();
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(64);
        let (conn, _cmd_rx) = crate::daemon_conn::DaemonConnection::connected_for_test();
        core.set_connection(conn, event_rx);
        (core, event_tx)
    }

    #[test]
    fn initial_view_is_sync_status() {
        let core = AppCore::new();
        assert!(matches!(
            core.view_model().active_view,
            ViewKind::SyncStatus
        ));
    }

    #[test]
    fn navigate_action_changes_view() {
        let mut core = AppCore::new();
        core.apply(Action::NavigateTo(ViewKind::GeneralSettings));
        assert!(matches!(
            core.view_model().active_view,
            ViewKind::GeneralSettings
        ));
    }

    #[tokio::test]
    async fn sync_started_event_updates_folder_status() {
        let (mut core, event_tx) = core_with_test_conn();
        let folder_id = Uuid::new_v4();
        core.state.accounts.push(crate::model::AccountView {
            id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            display_name: "test".to_string(),
            folders: vec![crate::model::FolderView {
                id: folder_id,
                display_name: "docs".to_string(),
                local_path: "/tmp/docs".to_string(),
                status: crate::model::FolderStatus::Idle,
                progress: None,
                errors: vec![],
            }],
        });
        event_tx
            .send(DaemonEvent::SyncStarted { folder_id })
            .await
            .unwrap();
        core.poll_events();
        let folder = core.view_model().accounts[0].folders[0].clone();
        assert_eq!(folder.status, crate::model::FolderStatus::Syncing);
    }
}

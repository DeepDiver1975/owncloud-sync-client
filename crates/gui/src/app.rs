use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent};

use crate::daemon_conn::DaemonConnection;
use crate::model::{AccountView, FolderStatus, FolderView, View};
use crate::tray::TrayHandle;

/// Carrier for the event receiver produced by `DaemonConnection::connect`.
/// Wrapped in Arc<Mutex<Option<...>>> so that `Message` can derive Clone.
pub type EventRxCarrier = Arc<Mutex<Option<mpsc::Receiver<DaemonEvent>>>>;

#[derive(Debug, Clone)]
pub struct App {
    pub daemon: DaemonConnection,
    pub accounts: Vec<AccountView>,
    pub active_view: View,
    pub tray: Option<TrayHandle>,
    pub window_visible: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            daemon: DaemonConnection::disconnected(),
            accounts: vec![],
            active_view: View::SyncStatus,
            tray: None,
            window_visible: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    DaemonEvent(DaemonEvent),
    DaemonDisconnected,
    NavigateTo(View),
    ToggleWindow,
    AddAccountUrlChanged(String),
    AddAccountSubmit,
    PickLocalFolderBrowse,
    PickLocalFolderPicked(Option<String>),
    PickLocalFolderSubmit,
    PickLocalFolderCancel,
    PauseFolder(Uuid),
    ResumeFolder(Uuid),
    ForceSyncFolder(Uuid),
    RemoveAccount(Uuid),
    OpenFolder(String),
    Quit,
    DaemonConnected(Option<(DaemonConnection, EventRxCarrier)>),
}

pub fn update(app: &mut App, message: Message) -> iced::Task<Message> {
    match message {
        Message::DaemonEvent(event) => handle_daemon_event(app, event),

        Message::DaemonDisconnected => {
            tracing::warn!("daemon disconnected");
            iced::Task::none()
        }

        Message::NavigateTo(view) => {
            app.active_view = view;
            iced::Task::none()
        }

        Message::ToggleWindow => {
            app.window_visible = !app.window_visible;
            iced::Task::none()
        }

        Message::AddAccountUrlChanged(url) => {
            if let View::AddAccount { url_input, .. } = &mut app.active_view {
                *url_input = url;
            }
            iced::Task::none()
        }

        Message::AddAccountSubmit => {
            if let View::AddAccount { url_input, error } = &mut app.active_view {
                let url = url_input.clone();
                if url.is_empty() {
                    *error = Some("Please enter a server URL".to_string());
                    return iced::Task::none();
                }
                let sent = app
                    .daemon
                    .send(DaemonCommand::AddAccount { url: url.clone() });
                if sent {
                    app.active_view = View::AddAccountWaiting {
                        account_id: Uuid::nil(),
                        url_input: url,
                    };
                } else {
                    *error = Some("Not connected to sync daemon".to_string());
                }
            }
            iced::Task::none()
        }

        Message::PickLocalFolderBrowse => {
            iced::Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .pick_folder()
                        .await
                        // rfd guarantees a valid path; on macOS/Windows paths are always
                        // UTF-8. On Linux with a non-UTF-8 locale this falls back to lossy
                        // conversion, which the daemon will reject as a non-existent path.
                        .map(|h| h.path().to_string_lossy().into_owned())
                },
                Message::PickLocalFolderPicked,
            )
        }

        Message::PickLocalFolderPicked(maybe_path) => {
            if let View::PickLocalFolder {
                local_path, error, ..
            } = &mut app.active_view
            {
                if let Some(path) = maybe_path {
                    *local_path = Some(path);
                    *error = None;
                }
                // None means user dismissed the picker — no change
            }
            iced::Task::none()
        }

        Message::PickLocalFolderSubmit => {
            if let View::PickLocalFolder {
                account_id,
                local_path,
                error,
                ..
            } = &mut app.active_view
            {
                match local_path.clone() {
                    Some(path) => {
                        let aid = *account_id;
                        *error = None;
                        app.daemon.send(DaemonCommand::SetAccountFolder {
                            account_id: aid,
                            local_path: path,
                        });
                    }
                    None => {
                        tracing::warn!("PickLocalFolderSubmit fired with no path selected");
                    }
                }
            }
            iced::Task::none()
        }

        Message::PickLocalFolderCancel => {
            if let View::PickLocalFolder { account_id, .. } = app.active_view {
                app.daemon.send(DaemonCommand::RemoveAccount { account_id });
            }
            app.active_view = View::SyncStatus;
            iced::Task::none()
        }

        Message::PauseFolder(folder_id) => {
            app.daemon.send(DaemonCommand::PauseFolder { folder_id });
            set_folder_status(app, folder_id, FolderStatus::Paused);
            iced::Task::none()
        }

        Message::ResumeFolder(folder_id) => {
            app.daemon.send(DaemonCommand::ResumeFolder { folder_id });
            set_folder_status(app, folder_id, FolderStatus::Idle);
            iced::Task::none()
        }

        Message::ForceSyncFolder(folder_id) => {
            app.daemon.send(DaemonCommand::TriggerSync { folder_id });
            iced::Task::none()
        }

        Message::RemoveAccount(account_id) => {
            app.daemon.send(DaemonCommand::RemoveAccount { account_id });
            app.accounts.retain(|a| a.id != account_id);
            app.active_view = View::SyncStatus;
            iced::Task::none()
        }

        Message::OpenFolder(path) => {
            tracing::info!("opening folder: {path}");
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&path).spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("explorer").arg(&path).spawn();
            iced::Task::none()
        }

        Message::Quit => {
            app.daemon.send(DaemonCommand::Quit);
            iced::exit()
        }

        Message::DaemonConnected(_) => iced::Task::none(),
    }
}

fn handle_daemon_event(app: &mut App, event: DaemonEvent) -> iced::Task<Message> {
    match event {
        DaemonEvent::Ready => {}

        DaemonEvent::SyncStarted { folder_id } => {
            if let Some(folder) = find_folder_mut(app, folder_id) {
                folder.status = FolderStatus::Syncing;
                folder.errors.clear();
            }
        }

        DaemonEvent::SyncProgress {
            folder_id,
            done,
            total,
        } => {
            if let Some(folder) = find_folder_mut(app, folder_id) {
                folder.progress = Some((done, total));
            }
        }

        DaemonEvent::SyncFinished {
            folder_id, errors, ..
        } => {
            if let Some(folder) = find_folder_mut(app, folder_id) {
                folder.progress = None;
                if errors.is_empty() {
                    folder.status = FolderStatus::Idle;
                    folder.errors.clear();
                } else {
                    folder.status = FolderStatus::Error;
                    folder.errors = errors;
                }
            }
        }

        DaemonEvent::FileStatusChanged { path, status } => {
            tracing::debug!("file status changed: {path} → {status}");
        }

        DaemonEvent::AccountStateChanged { account_id, state } => {
            tracing::debug!("account state changed: {account_id} → {state}");
        }

        DaemonEvent::AccountAddStarted { account_id } => {
            if let View::AddAccountWaiting {
                account_id: stored, ..
            } = &mut app.active_view
            {
                *stored = account_id;
            }
        }

        DaemonEvent::AccountAddFailed {
            account_id: _,
            reason,
        } => {
            if let View::AddAccountWaiting { url_input, .. } = &app.active_view {
                let url = url_input.clone();
                app.active_view = View::AddAccount {
                    url_input: url,
                    error: Some(reason),
                };
            } else {
                tracing::warn!(
                    "AccountAddFailed received but not in AddAccountWaiting view: {reason}"
                );
            }
        }

        DaemonEvent::AccountAddCompleted {
            account_id,
            display_name,
            url,
            ..
        } => {
            app.accounts.push(AccountView {
                id: account_id,
                url: url.clone(),
                display_name: display_name.clone(),
                folders: vec![],
            });
            app.active_view = View::PickLocalFolder {
                account_id,
                display_name,
                url,
                local_path: None,
                error: None,
            };
        }

        DaemonEvent::AccountFolderAdded {
            account_id,
            folder_id,
            local_path,
            display_name,
        } => {
            if let Some(account) = app.accounts.iter_mut().find(|a| a.id == account_id) {
                account.folders.push(FolderView {
                    id: folder_id,
                    display_name,
                    local_path,
                    status: FolderStatus::Idle,
                    progress: None,
                    errors: vec![],
                });
            }
            if matches!(&app.active_view, View::PickLocalFolder { account_id: aid, .. } if *aid == account_id)
            {
                app.active_view = View::SyncStatus;
            }
        }

        DaemonEvent::AccountSetFolderFailed { account_id, reason } => {
            if let View::PickLocalFolder {
                account_id: aid,
                error,
                ..
            } = &mut app.active_view
            {
                if *aid == account_id {
                    *error = Some(reason);
                }
            }
        }

        DaemonEvent::AccountSnapshot { accounts } => {
            app.accounts = accounts
                .into_iter()
                .map(|a| AccountView {
                    id: a.account_id,
                    url: a.url,
                    display_name: a.display_name,
                    folders: a
                        .folders
                        .into_iter()
                        .map(|f| FolderView {
                            id: f.folder_id,
                            display_name: f.display_name,
                            local_path: f.local_path,
                            status: match f.status.as_str() {
                                "syncing" => FolderStatus::Syncing,
                                "paused" => FolderStatus::Paused,
                                _ => FolderStatus::Idle,
                            },
                            progress: None,
                            errors: vec![],
                        })
                        .collect(),
                })
                .collect();
        }
    }

    iced::Task::none()
}

fn find_folder_mut(app: &mut App, folder_id: Uuid) -> Option<&mut FolderView> {
    app.accounts
        .iter_mut()
        .flat_map(|a| a.folders.iter_mut())
        .find(|f| f.id == folder_id)
}

fn set_folder_status(app: &mut App, folder_id: Uuid, status: FolderStatus) {
    if let Some(folder) = find_folder_mut(app, folder_id) {
        folder.status = status;
    }
}

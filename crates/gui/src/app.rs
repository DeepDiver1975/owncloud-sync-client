use rust_i18n::t;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use daemon::gui_ipc::protocol::{DaemonCommand, DaemonEvent, SpaceSelection};

use crate::daemon_conn::DaemonConnection;
use crate::model::{AccountView, FolderStatus, FolderView, SpaceInfo, View};
use crate::tray::TrayHandle;

pub type EventRxCarrier = Arc<Mutex<Option<mpsc::Receiver<DaemonEvent>>>>;

#[derive(Debug, Clone)]
pub struct App {
    pub daemon: DaemonConnection,
    pub accounts: Vec<AccountView>,
    pub active_view: View,
    pub tray: Option<TrayHandle>,
    pub window_visible: bool,
    pub language: crate::model::Language,
    pub gui_config_path: PathBuf,
}

impl Default for App {
    fn default() -> Self {
        Self {
            daemon: DaemonConnection::disconnected(),
            accounts: vec![],
            active_view: View::SyncStatus,
            tray: None,
            window_visible: true,
            language: crate::model::Language::En,
            gui_config_path: PathBuf::new(),
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
    // Space selection (setup wizard)
    ToggleSpaceSelection {
        account_id: Uuid,
        space_id: String,
        selected: bool,
    },
    PickSpacesNext {
        account_id: Uuid,
    },
    PickRootFolderBrowse,
    PickRootFolderPicked(Option<String>),
    PickRootFolderSubmit {
        account_id: Uuid,
    },
    // Account settings: add space
    AddSpaceClicked {
        account_id: Uuid,
    },
    // Runtime space events
    AcceptDiscoveredSpace {
        account_id: Uuid,
        space_id: String,
        suggested_path: String,
    },
    DismissDiscoveredSpace {
        account_id: Uuid,
        space_id: String,
    },
    // Folder actions
    PauseFolder(Uuid),
    ResumeFolder(Uuid),
    ForceSyncFolder(Uuid),
    RemoveAccount(Uuid),
    OpenFolder(String),
    Quit,
    ShowAbout,
    OpenUrl(String),
    DaemonConnected(Option<(DaemonConnection, EventRxCarrier)>),
    LanguageChanged(crate::model::Language),
    #[cfg(target_os = "macos")]
    ApplyAppIcon,
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
                let normalized = strip_url_schema(url_input.trim()).to_string();
                *url_input = normalized.clone();
                if normalized.is_empty() {
                    *error = Some(t!("error_enter_url").to_string());
                    return iced::Task::none();
                }
                let sent = app.daemon.send(DaemonCommand::AddAccount {
                    url: normalized.clone(),
                });
                if sent {
                    app.active_view = View::AddAccountWaiting {
                        account_id: Uuid::nil(),
                        url_input: normalized,
                    };
                } else {
                    *error = Some(t!("error_not_connected").to_string());
                }
            }
            iced::Task::none()
        }

        Message::ToggleSpaceSelection {
            account_id: _,
            space_id,
            selected,
        } => {
            if let View::PickSpaces { selected: sel, .. } = &mut app.active_view {
                if selected {
                    sel.insert(space_id);
                } else {
                    sel.remove(&space_id);
                }
            }
            iced::Task::none()
        }

        Message::PickSpacesNext { account_id } => {
            let has_folders = app
                .accounts
                .iter()
                .find(|a| a.id == account_id)
                .map(|a| !a.folders.is_empty())
                .unwrap_or(false);

            if has_folders {
                // Add-space flow: derive root and send AddAccountSpace per selection
                if let View::PickSpaces {
                    spaces, selected, ..
                } = &app.active_view
                {
                    let existing_paths: Vec<String> = app
                        .accounts
                        .iter()
                        .find(|a| a.id == account_id)
                        .map(|a| a.folders.iter().map(|f| f.local_path.clone()).collect())
                        .unwrap_or_default();
                    let root = derive_root(&existing_paths);
                    for space in spaces.iter().filter(|s| selected.contains(&s.id)) {
                        let local_path = format!("{}/{}", root.trim_end_matches('/'), space.name);
                        app.daemon.send(DaemonCommand::AddAccountSpace {
                            account_id,
                            space_id: space.id.clone(),
                            local_path,
                        });
                    }
                    app.active_view = View::AccountSettings(account_id);
                }
            } else {
                // Setup flow: go to PickRootFolder
                if let View::PickSpaces {
                    spaces, selected, ..
                } = &app.active_view
                {
                    let selected_spaces: Vec<SpaceInfo> = spaces
                        .iter()
                        .filter(|s| selected.contains(&s.id))
                        .cloned()
                        .collect();
                    app.active_view = View::PickRootFolder {
                        account_id,
                        spaces: selected_spaces,
                        local_path: None,
                        error: None,
                    };
                }
            }
            iced::Task::none()
        }

        Message::PickRootFolderBrowse => iced::Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .pick_folder()
                    .await
                    .map(|h| h.path().to_string_lossy().into_owned())
            },
            Message::PickRootFolderPicked,
        ),

        Message::PickRootFolderPicked(maybe_path) => {
            if let View::PickRootFolder {
                local_path, error, ..
            } = &mut app.active_view
            {
                if let Some(path) = maybe_path {
                    *local_path = Some(path);
                    *error = None;
                }
            }
            iced::Task::none()
        }

        Message::PickRootFolderSubmit { account_id } => {
            if let View::PickRootFolder {
                spaces, local_path, ..
            } = &app.active_view
            {
                if let Some(root) = local_path.clone() {
                    let selections: Vec<SpaceSelection> = spaces
                        .iter()
                        .map(|s| SpaceSelection {
                            space_id: s.id.clone(),
                            display_name: s.name.clone(),
                        })
                        .collect();
                    app.daemon.send(DaemonCommand::SetAccountFolders {
                        account_id,
                        root_path: root,
                        spaces: selections,
                    });
                }
            }
            iced::Task::none()
        }

        Message::AddSpaceClicked { account_id } => {
            app.daemon.send(DaemonCommand::ListSpaces { account_id });
            iced::Task::none()
        }

        Message::AcceptDiscoveredSpace {
            account_id,
            space_id,
            suggested_path,
        } => {
            app.daemon.send(DaemonCommand::AddAccountSpace {
                account_id,
                space_id,
                local_path: suggested_path,
            });
            iced::Task::none()
        }

        Message::DismissDiscoveredSpace {
            account_id,
            space_id,
        } => {
            app.daemon.send(DaemonCommand::DismissSpace {
                account_id,
                space_id,
            });
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
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&path).spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("explorer").arg(&path).spawn();
            iced::Task::none()
        }

        Message::LanguageChanged(lang) => {
            rust_i18n::set_locale(lang.as_locale());
            if let Some(tray) = &app.tray {
                tray.rebuild_menu(&t!("tray_open"), &t!("tray_about"), &t!("tray_quit"));
            }
            let path = app.gui_config_path.clone();
            app.language = lang.clone();
            let cfg = crate::gui_config::GuiConfig {
                language: Some(lang),
            };
            iced::Task::perform(
                async move {
                    cfg.save(&path).ok();
                },
                |_| Message::NavigateTo(View::GeneralSettings),
            )
        }

        Message::Quit => {
            app.daemon.send(DaemonCommand::Quit);
            iced::exit()
        }

        Message::ShowAbout => {
            app.window_visible = true;
            app.active_view = View::About;
            iced::Task::none()
        }

        Message::OpenUrl(url) => {
            tracing::info!("opening url: {url}");
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&url).spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "", &url])
                .spawn();
            iced::Task::none()
        }

        Message::DaemonConnected(_) => iced::Task::none(),

        #[cfg(target_os = "macos")]
        Message::ApplyAppIcon => {
            crate::macos_icon::set_app_icon();
            iced::Task::none()
        }
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
                display_name,
                folders: vec![],
            });
            // Request space list; PickSpaces shown when SpacesListed arrives
            app.daemon.send(DaemonCommand::ListSpaces { account_id });
        }

        DaemonEvent::SpacesListed { account_id, spaces } => {
            let space_infos: Vec<SpaceInfo> = spaces
                .into_iter()
                .map(|s| SpaceInfo {
                    id: s.id,
                    name: s.name,
                    drive_type: s.drive_type,
                })
                .collect();

            let is_setup = app
                .accounts
                .iter()
                .find(|a| a.id == account_id)
                .map(|a| a.folders.is_empty())
                .unwrap_or(true);

            if is_setup {
                let selected: HashSet<String> = space_infos
                    .iter()
                    .filter(|s| s.drive_type == "personal")
                    .map(|s| s.id.clone())
                    .collect();
                app.active_view = View::PickSpaces {
                    account_id,
                    spaces: space_infos,
                    selected,
                    error: None,
                };
            } else {
                let synced_space_ids: HashSet<String> = app
                    .accounts
                    .iter()
                    .find(|a| a.id == account_id)
                    .map(|a| a.folders.iter().map(|f| f.space_id.clone()).collect())
                    .unwrap_or_default();
                let available: Vec<SpaceInfo> = space_infos
                    .into_iter()
                    .filter(|s| !synced_space_ids.contains(&s.id))
                    .collect();
                app.active_view = View::PickSpaces {
                    account_id,
                    spaces: available,
                    selected: HashSet::new(),
                    error: None,
                };
            }
        }

        DaemonEvent::AccountSpaceFailed { account_id, reason } => match &mut app.active_view {
            View::PickSpaces {
                account_id: aid,
                error,
                ..
            } if *aid == account_id => {
                *error = Some(reason);
            }
            View::PickRootFolder {
                account_id: aid,
                error,
                ..
            } if *aid == account_id => {
                *error = Some(reason);
            }
            _ => {
                tracing::warn!("AccountSpaceFailed for {account_id}: {reason}");
            }
        },

        DaemonEvent::AccountFolderAdded {
            account_id,
            folder_id,
            space_id,
            local_path,
            display_name,
        } => {
            if let Some(account) = app.accounts.iter_mut().find(|a| a.id == account_id) {
                account.folders.push(FolderView {
                    id: folder_id,
                    space_id,
                    display_name,
                    local_path,
                    status: FolderStatus::Idle,
                    progress: None,
                    errors: vec![],
                });
            }
            if matches!(&app.active_view, View::PickRootFolder { account_id: aid, .. } if *aid == account_id)
            {
                app.active_view = View::SyncStatus;
            }
        }

        DaemonEvent::SpaceDiscovered {
            account_id,
            space_id: _,
            space_name,
            suggested_path: _,
        } => {
            // TODO: show OS desktop notification via notify-rust with Accept/Dismiss actions.
            // For now, log. AcceptDiscoveredSpace / DismissDiscoveredSpace messages are wired
            // and ready; the notification integration is a follow-up task.
            tracing::info!(
                "New space '{space_name}' discovered for account {account_id} (notification pending)"
            );
        }

        DaemonEvent::SpaceRemoved {
            account_id,
            folder_id,
            space_name,
            local_path,
        } => {
            if let Some(account) = app.accounts.iter_mut().find(|a| a.id == account_id) {
                account.folders.retain(|f| f.id != folder_id);
            }
            tracing::info!("Space '{space_name}' removed; local files remain at {local_path}");
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
                            space_id: f.space_id,
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

fn strip_url_schema(s: &str) -> &str {
    for prefix in ["https://", "http://"] {
        if let Some(rest) = s.get(..prefix.len()) {
            if rest.eq_ignore_ascii_case(prefix) {
                return &s[prefix.len()..];
            }
        }
    }
    s
}

fn derive_root(existing_paths: &[String]) -> String {
    existing_paths
        .first()
        .and_then(|p| std::path::Path::new(p).parent())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join("ownCloud").to_string_lossy().into_owned())
                .unwrap_or_else(|| "/tmp/ownCloud".to_string())
        })
}

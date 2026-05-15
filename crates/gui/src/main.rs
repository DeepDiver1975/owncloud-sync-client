use std::sync::Arc;
use tokio::sync::Mutex;

rust_i18n::i18n!("locales", fallback = "en");

use gui::app::{update, App, EventRxCarrier, Message};
use gui::daemon_conn::DaemonConnection;
use gui::gui_config::GuiConfig;
use gui::i18n::detect_system_language;
use gui::model::View;
use gui::spawn::ensure_daemon_running;
use gui::subscription::next_message;
use gui::theme;
use gui::tray::TrayHandle;

use daemon::paths::{platform_config_dir, platform_gui_socket_path};

use iced::futures::SinkExt;
use iced::widget::{column, container, row, text};
use iced::{Element, Length, Subscription, Task};
use rust_i18n::t;

#[cfg(feature = "test-accessibility")]
fn init_accessibility() {
    use accesskit::{
        ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId, Role,
        Tree, TreeId, TreeUpdate,
    };
    use accesskit_unix::Adapter;

    struct OcsyncActivation;
    impl ActivationHandler for OcsyncActivation {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            let root_id = NodeId(0);
            let mut root = Node::new(Role::Window);
            root.set_label("ocsync");
            Some(TreeUpdate {
                nodes: vec![(root_id, root)],
                tree: Some(Tree::new(root_id)),
                tree_id: TreeId::ROOT,
                focus: root_id,
            })
        }
    }

    struct OcsyncAction;
    impl ActionHandler for OcsyncAction {
        fn do_action(&mut self, _request: ActionRequest) {}
    }

    struct OcsyncDeactivation;
    impl DeactivationHandler for OcsyncDeactivation {
        fn deactivate_accessibility(&mut self) {}
    }

    let adapter = Adapter::new(OcsyncActivation, OcsyncAction, OcsyncDeactivation);
    std::mem::forget(adapter);
}

fn main() -> iced::Result {
    #[cfg(feature = "test-accessibility")]
    init_accessibility();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    iced::application("ownCloud Sync", IcedApp::update, IcedApp::view)
        .theme(|_| theme::app_theme())
        .window(iced::window::Settings {
            size: iced::Size::new(800.0, 480.0),
            min_size: Some(iced::Size::new(600.0, 400.0)),
            ..Default::default()
        })
        .subscription(IcedApp::subscription)
        .run_with(IcedApp::init)
}

struct IcedApp {
    app: App,
    event_rx: EventRxCarrier,
}

impl IcedApp {
    fn init() -> (Self, Task<Message>) {
        let gui_config_path = platform_config_dir().join("gui-config.toml");
        let mut gui_config = GuiConfig::load_or_default(&gui_config_path);

        let language = match gui_config.language.clone() {
            Some(lang) => lang,
            None => {
                let detected = detect_system_language();
                gui_config.language = Some(detected.clone());
                gui_config.save(&gui_config_path).ok();
                detected
            }
        };
        rust_i18n::set_locale(language.as_locale());

        let tray = TrayHandle::build()
            .map_err(|e| tracing::warn!("tray icon unavailable: {e}"))
            .ok();

        let event_rx: EventRxCarrier = Arc::new(Mutex::new(None));
        let init_task = Task::perform(
            async {
                let socket = platform_gui_socket_path();
                if let Err(e) = ensure_daemon_running(&socket).await {
                    tracing::warn!("daemon not available: {e}");
                    return None;
                }
                match DaemonConnection::connect(&socket).await {
                    Ok((conn, rx)) => {
                        let carrier: EventRxCarrier = Arc::new(Mutex::new(Some(rx)));
                        Some((conn, carrier))
                    }
                    Err(e) => {
                        tracing::error!("failed to connect to daemon: {e}");
                        None
                    }
                }
            },
            Message::DaemonConnected,
        );
        (
            Self {
                app: App {
                    tray,
                    language,
                    gui_config_path,
                    ..App::default()
                },
                event_rx,
            },
            init_task,
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if let Message::DaemonConnected(Some((conn, carrier))) = &message {
            self.app.daemon = conn.clone();
            let carrier = carrier.clone();
            let our_rx = self.event_rx.clone();
            return Task::perform(
                async move {
                    let mut guard = carrier.lock().await;
                    let mut ours = our_rx.lock().await;
                    *ours = guard.take();
                },
                |_| Message::NavigateTo(gui::model::View::SyncStatus),
            );
        }

        if matches!(message, Message::DaemonDisconnected) {
            let socket = platform_gui_socket_path();
            return Task::perform(
                async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if ensure_daemon_running(&socket).await.is_err() {
                        return None;
                    }
                    match DaemonConnection::connect(&socket).await {
                        Ok((conn, rx)) => {
                            let carrier: EventRxCarrier = Arc::new(Mutex::new(Some(rx)));
                            Some((conn, carrier))
                        }
                        Err(e) => {
                            tracing::warn!("daemon reconnect failed: {e}");
                            None
                        }
                    }
                },
                Message::DaemonConnected,
            );
        }

        update(&mut self.app, message)
    }

    fn view(&self) -> Element<'_, Message> {
        // Title bar
        let title_bar = container(
            row![
                theme::owncloud_icon(),
                text("ownCloud Sync")
                    .size(12)
                    .style(theme::colored_text(theme::TEXT_PRIMARY)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .padding([6, 12]),
        )
        .width(Length::Fill)
        .style(theme::sidebar_style);

        // Sidebar nav
        let is_sync = matches!(
            self.app.active_view,
            View::SyncStatus | View::FolderErrors { .. }
        );
        let is_add = matches!(
            self.app.active_view,
            View::AddAccount { .. }
                | View::AddAccountWaiting { .. }
                | View::PickSpaces { .. }
                | View::PickRootFolder { .. }
        );
        let is_settings = matches!(
            self.app.active_view,
            View::GeneralSettings | View::AccountSettings(_)
        );
        let is_about = matches!(self.app.active_view, View::About);

        let nav_sync = iced::widget::button(text("☁ Sync Status").size(12).style(
            theme::colored_text(if is_sync {
                theme::ACCENT
            } else {
                theme::TEXT_SECONDARY
            }),
        ))
        .on_press(Message::NavigateTo(View::SyncStatus))
        .width(Length::Fill)
        .padding([7, 9])
        .style(if is_sync {
            theme::nav_active_style
        } else {
            theme::nav_button_style
        });

        let nav_add = iced::widget::button(text("+ Add Account").size(12).style(
            theme::colored_text(if is_add {
                theme::ACCENT
            } else {
                theme::TEXT_SECONDARY
            }),
        ))
        .on_press(Message::NavigateTo(View::AddAccount {
            url_input: String::new(),
            error: None,
        }))
        .width(Length::Fill)
        .padding([7, 9])
        .style(if is_add {
            theme::nav_active_style
        } else {
            theme::nav_button_style
        });

        let nav_settings = iced::widget::button(text("⚙ Settings").size(12).style(
            theme::colored_text(if is_settings {
                theme::ACCENT
            } else {
                theme::TEXT_SECONDARY
            }),
        ))
        .on_press(Message::NavigateTo(View::GeneralSettings))
        .width(Length::Fill)
        .padding([7, 9])
        .style(if is_settings {
            theme::nav_active_style
        } else {
            theme::nav_button_style
        });

        let nav_about = iced::widget::button(text("ℹ About").size(12).style(
            theme::colored_text(if is_about {
                theme::ACCENT
            } else {
                theme::TEXT_SECONDARY
            }),
        ))
        .on_press(Message::NavigateTo(View::About))
        .width(Length::Fill)
        .padding([7, 9])
        .style(if is_about {
            theme::nav_active_style
        } else {
            theme::nav_button_style
        });

        let sidebar = container(
            column![nav_sync, nav_add, nav_settings, nav_about]
                .spacing(2)
                .padding([8, 6]),
        )
        .width(156)
        .height(Length::Fill)
        .style(theme::sidebar_style);

        // Content
        let content: Element<Message> = match &self.app.active_view {
            View::SyncStatus => gui::views::sync_status::sync_status_view(&self.app.accounts),
            View::AddAccount { url_input, error } => {
                gui::views::add_account::add_account_view(url_input, error.as_deref())
            }
            View::AccountSettings(account_id) => {
                if let Some(account) = self.app.accounts.iter().find(|a| &a.id == account_id) {
                    gui::views::account_settings::account_settings_view(account)
                } else {
                    container(
                        text(t!("account_not_found").to_string()).style(theme::colored_text(theme::TEXT_MUTED)),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
                }
            }
            View::AddAccountWaiting { .. } => {
                gui::views::add_account_waiting::add_account_waiting_view()
            }
            View::PickSpaces {
                account_id,
                spaces,
                selected,
                error,
            } => gui::views::pick_spaces::pick_spaces_view(
                *account_id,
                spaces,
                selected,
                error.as_deref(),
                "Choose spaces to sync",
            ),
            View::PickRootFolder {
                account_id,
                spaces,
                local_path,
                error,
            } => gui::views::pick_root_folder::pick_root_folder_view(
                *account_id,
                spaces,
                local_path.as_deref(),
                error.as_deref(),
            ),
            View::GeneralSettings => gui::views::general_settings::general_settings_view(&self.app.language),
            View::About => gui::views::about::about_view(),
            View::FolderErrors {
                account_id,
                folder_id,
            } => {
                if let Some(account) = self.app.accounts.iter().find(|a| &a.id == account_id) {
                    gui::views::folder_errors::folder_errors_view(account, *folder_id)
                } else {
                    gui::views::sync_status::sync_status_view(&self.app.accounts)
                }
            }
        };

        let body = row![sidebar, content].height(Length::Fill);

        container(column![title_bar, body])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::content_style)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let rx = self.event_rx.clone();
        let daemon_sub = Subscription::run_with_id(
            "daemon-events",
            iced::stream::channel(16, move |mut output| async move {
                loop {
                    let msg = {
                        let mut guard = rx.lock().await;
                        if let Some(receiver) = guard.as_mut() {
                            let m = next_message(receiver).await;
                            if matches!(m, Some(Message::DaemonDisconnected)) {
                                *guard = None;
                            }
                            m
                        } else {
                            drop(guard);
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            None
                        }
                    };
                    if let Some(m) = msg {
                        let _ = output.send(m).await;
                    }
                }
            }),
        );

        let tray_sub = self
            .app
            .tray
            .as_ref()
            .map(|t| t.tray_events())
            .unwrap_or(Subscription::none());

        Subscription::batch([daemon_sub, tray_sub])
    }
}

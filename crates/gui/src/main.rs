use std::sync::Arc;
use tokio::sync::Mutex;

use daemon::paths::platform_gui_socket_path;
use gui::app::{App, Message};
use gui::model::ViewKind;
use gui_core::{Action, AppCore};

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Subscription, Task};

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    iced::application("ownCloud Sync", IcedApp::update, IcedApp::view)
        .subscription(IcedApp::subscription)
        .run_with(IcedApp::init)
}

struct IcedApp {
    app: App,
}

impl IcedApp {
    fn init() -> (Self, Task<Message>) {
        let socket = platform_gui_socket_path();
        let init_task = Task::perform(
            async move {
                let core = AppCore::init(&socket).await;
                let vm = core.view_model();
                let core = Arc::new(Mutex::new(core));
                (core, vm)
            },
            |(core, vm)| Message::CoreInitialized(core, vm),
        );
        (
            Self {
                app: App::default(),
            },
            init_task,
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        gui::app::update(&mut self.app, message)
    }

    fn view(&self) -> Element<'_, Message> {
        let vm = &self.app.vm;
        let content: Element<Message> = match &vm.active_view {
            ViewKind::SyncStatus => gui::views::sync_status::sync_status_view(&vm.accounts),
            ViewKind::AddAccount { url_input, error } => {
                gui::views::add_account::add_account_view(url_input, error.as_deref())
            }
            ViewKind::AccountSettings(account_id) => {
                if let Some(account) = vm.accounts.iter().find(|a| &a.id == account_id) {
                    gui::views::account_settings::account_settings_view(account)
                } else {
                    container(text("Account not found"))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                }
            }
            ViewKind::AddAccountWaiting { .. } => {
                gui::views::add_account_waiting::add_account_waiting_view()
            }
            ViewKind::GeneralSettings => gui::views::general_settings::general_settings_view(),
        };

        let nav = row![
            iced::widget::button("Sync Status")
                .on_press(Message::Action(Action::NavigateTo(ViewKind::SyncStatus)))
                .padding(8),
            iced::widget::button("Add Account")
                .on_press(Message::Action(Action::NavigateTo(ViewKind::AddAccount {
                    url_input: String::new(),
                    error: None,
                })))
                .padding(8),
            iced::widget::button("Settings")
                .on_press(Message::Action(Action::NavigateTo(
                    ViewKind::GeneralSettings
                )))
                .padding(8),
        ]
        .spacing(4)
        .padding(8);

        column![nav, content].into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let core = self.app.core.clone();
        Subscription::run_with_id(
            "daemon-events",
            iced::stream::channel(16, move |mut output| async move {
                use iced::futures::SinkExt;
                loop {
                    {
                        let mut guard = core.lock().await;
                        if guard.poll_events() {
                            let vm = guard.view_model();
                            let _ = output.send(Message::ViewModelUpdated(vm)).await;
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }),
        )
    }
}

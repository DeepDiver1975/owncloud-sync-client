use std::sync::Arc;
use tokio::sync::Mutex;

use gui::app::{update, App, EventRxCarrier, Message};
use gui::daemon_conn::DaemonConnection;
use gui::model::View;
use gui::spawn::ensure_daemon_running;
use gui::subscription::next_message;

use iced::futures::SinkExt;
use iced::widget::{column, container, row, text};
use iced::{Element, Length, Subscription, Task};

const SOCKET_PATH: &str = "/tmp/ocsyncd.sock";

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
    event_rx: EventRxCarrier,
}

impl IcedApp {
    fn init() -> (Self, Task<Message>) {
        let event_rx: EventRxCarrier = Arc::new(Mutex::new(None));
        let init_task = Task::perform(
            async {
                let socket = std::path::Path::new(SOCKET_PATH);
                if let Err(e) = ensure_daemon_running(socket).await {
                    tracing::warn!("daemon not available: {e}");
                    return None;
                }
                match DaemonConnection::connect(socket).await {
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
                app: App::default(),
                event_rx,
            },
            init_task,
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if let Message::DaemonConnected(Some((conn, carrier))) = &message {
            self.app.daemon = conn.clone();
            // Swap the carrier's inner receiver into our shared Arc
            let carrier = carrier.clone();
            let our_rx = self.event_rx.clone();
            return Task::perform(
                async move {
                    let mut guard = carrier.lock().await;
                    let mut ours = our_rx.lock().await;
                    *ours = guard.take();
                },
                |_| Message::DaemonDisconnected,
            );
        }
        update(&mut self.app, message)
    }

    fn view(&self) -> Element<Message> {
        let content: Element<Message> = match &self.app.active_view {
            View::SyncStatus => gui::views::sync_status::sync_status_view(&self.app.accounts),
            View::AddAccount { url_input, error } => {
                gui::views::add_account::add_account_view(url_input, error.as_deref())
            }
            View::AccountSettings(account_id) => {
                if let Some(account) = self.app.accounts.iter().find(|a| &a.id == account_id) {
                    gui::views::account_settings::account_settings_view(account)
                } else {
                    container(text("Account not found"))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                }
            }
            View::GeneralSettings => gui::views::general_settings::general_settings_view(),
        };

        let nav = row![
            iced::widget::button("Sync Status")
                .on_press(Message::NavigateTo(View::SyncStatus))
                .padding(8),
            iced::widget::button("Add Account")
                .on_press(Message::NavigateTo(View::AddAccount {
                    url_input: String::new(),
                    error: None,
                }))
                .padding(8),
            iced::widget::button("Settings")
                .on_press(Message::NavigateTo(View::GeneralSettings))
                .padding(8),
        ]
        .spacing(4)
        .padding(8);

        column![nav, content].into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let rx = self.event_rx.clone();
        Subscription::run_with_id(
            "daemon-events",
            iced::stream::channel(16, move |mut output| async move {
                loop {
                    let msg = {
                        let mut guard = rx.lock().await;
                        if let Some(receiver) = guard.as_mut() {
                            next_message(receiver).await
                        } else {
                            drop(guard);
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            None
                        }
                    };
                    if let Some(m) = msg {
                        let is_disconnect = matches!(m, Message::DaemonDisconnected);
                        let _ = output.send(m).await;
                        if is_disconnect {
                            // Wait before attempting reconnect
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            }),
        )
    }
}

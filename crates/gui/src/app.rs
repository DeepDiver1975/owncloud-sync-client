use std::sync::Arc;
use tokio::sync::Mutex;

use gui_core::{Action, AppCore, BackendCommand, ViewModel};

use crate::tray::TrayHandle;

#[derive(Debug, Clone)]
pub struct App {
    pub core: Arc<Mutex<AppCore>>,
    pub vm: ViewModel,
    pub tray: Option<TrayHandle>,
}

impl Default for App {
    fn default() -> Self {
        let core = AppCore::new();
        let vm = core.view_model();
        Self {
            core: Arc::new(Mutex::new(core)),
            vm,
            tray: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Action(Action),
    ViewModelUpdated(ViewModel),
    Quit,
}

pub fn update(app: &mut App, message: Message) -> iced::Task<Message> {
    match message {
        Message::Action(action) => {
            let core = app.core.clone();
            iced::Task::perform(
                async move {
                    let mut guard = core.lock().await;
                    let cmds = guard.apply(action);
                    let vm = guard.view_model();
                    (vm, cmds)
                },
                |(vm, cmds)| {
                    for cmd in &cmds {
                        if let BackendCommand::OpenFolder(path) = cmd {
                            open_folder(path);
                        }
                    }
                    if cmds.iter().any(|c| matches!(c, BackendCommand::Quit)) {
                        return Message::Quit;
                    }
                    Message::ViewModelUpdated(vm)
                },
            )
        }
        Message::ViewModelUpdated(vm) => {
            app.vm = vm;
            iced::Task::none()
        }
        Message::Quit => iced::exit(),
    }
}

fn open_folder(path: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

use iced::{
    widget::{button, column, container, row, text, Column},
    Alignment, Element, Length,
};

use gui_core::Action;

use crate::app::Message;
use crate::model::{AccountView, FolderStatus, ViewKind};

const PADDING: u16 = 12;
const SPACING: u16 = 8;

pub fn sync_status_view(accounts: &[AccountView]) -> Element<'_, Message> {
    if accounts.is_empty() {
        return empty_state_view();
    }

    let mut col = Column::new().padding(PADDING).spacing(SPACING * 2);

    for account in accounts {
        col = col.push(account_section(account));
    }

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn empty_state_view() -> Element<'static, Message> {
    let content = column![
        text("No accounts configured").size(18),
        text("Add your first ownCloud account to start syncing.").size(14),
        button("Add Account")
            .on_press(Message::Action(Action::NavigateTo(ViewKind::AddAccount {
                url_input: String::new(),
                error: None,
            })))
            .padding(PADDING),
    ]
    .spacing(SPACING)
    .align_x(Alignment::Center);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn account_section(account: &AccountView) -> Element<'_, Message> {
    let header = text(&account.url).size(16);

    let mut folders_col = Column::new().spacing(SPACING);
    for folder in &account.folders {
        folders_col = folders_col.push(folder_row(folder));
    }

    column![header, folders_col].spacing(SPACING).into()
}

fn folder_row(folder: &crate::model::FolderView) -> Element<'_, Message> {
    let status_sym = status_symbol(&folder.status);

    let name_and_path = column![
        text(&folder.display_name).size(14),
        text(truncate_path(&folder.local_path, 40)).size(12),
    ]
    .spacing(2);

    let progress_text: Element<Message> = if let Some((done, total)) = folder.progress {
        let pct = done.checked_div(total).unwrap_or(0) * 100;
        text(format!("{pct}%")).size(12).into()
    } else {
        text("").size(12).into()
    };

    let action_buttons = folder_action_buttons(folder);

    row![
        status_sym,
        name_and_path,
        progress_text,
        iced::widget::horizontal_space(),
        action_buttons,
    ]
    .spacing(SPACING)
    .align_y(Alignment::Center)
    .padding(PADDING / 2)
    .into()
}

fn status_symbol(status: &FolderStatus) -> Element<'static, Message> {
    let label = match status {
        FolderStatus::Idle => "●",
        FolderStatus::Syncing => "↻",
        FolderStatus::Error => "✕",
        FolderStatus::Paused => "⏸",
    };
    text(label).size(16).into()
}

fn folder_action_buttons(folder: &crate::model::FolderView) -> Element<'_, Message> {
    let folder_id = folder.id;
    let local_path = folder.local_path.clone();

    let pause_resume: Element<Message> = match folder.status {
        FolderStatus::Paused => button("Resume")
            .on_press(Message::Action(Action::ResumeFolder(folder_id)))
            .padding(PADDING / 2)
            .into(),
        _ => button("Pause")
            .on_press(Message::Action(Action::PauseFolder(folder_id)))
            .padding(PADDING / 2)
            .into(),
    };

    let sync_btn = button("Sync now")
        .on_press(Message::Action(Action::ForceSyncFolder(folder_id)))
        .padding(PADDING / 2);

    let open_btn = button("Open folder")
        .on_press(Message::Action(Action::OpenFolder(local_path)))
        .padding(PADDING / 2);

    row![pause_resume, sync_btn, open_btn]
        .spacing(SPACING / 2)
        .into()
}

pub fn truncate_path(path: &str, max_chars: usize) -> String {
    if path.len() <= max_chars {
        return path.to_string();
    }
    let half = max_chars / 2 - 1;
    format!("{}…{}", &path[..half], &path[path.len() - half..])
}

#[cfg(test)]
mod tests {
    use super::truncate_path;

    #[test]
    fn short_path_unchanged() {
        assert_eq!(truncate_path("/home/user/docs", 40), "/home/user/docs");
    }

    #[test]
    fn long_path_is_truncated() {
        let long = "/home/user/very/deeply/nested/path/that/exceeds/the/limit/file.txt";
        let result = truncate_path(long, 40);
        assert!(result.len() <= 41, "truncated path too long: {result}");
        assert!(
            result.contains('…'),
            "truncated path should contain ellipsis"
        );
    }
}

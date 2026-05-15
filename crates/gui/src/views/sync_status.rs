use iced::{
    widget::{button, column, container, progress_bar, row, text, Column, Space},
    Alignment, Element, Length,
};

use rust_i18n::t;

use crate::app::Message;
use crate::model::{AccountView, FolderStatus, View};
use crate::theme::{
    self, card_style, icon_button_style, section_header_style, status_badge_style, status_color,
};

pub fn sync_status_view(accounts: &[AccountView]) -> Element<'_, Message> {
    if accounts.is_empty() {
        return empty_state_view();
    }

    let mut col = Column::new().spacing(16);
    for account in accounts {
        col = col.push(account_section(account));
    }

    container(iced::widget::scrollable(col).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([16, 20])
        .into()
}

fn empty_state_view() -> Element<'static, Message> {
    let cloud = theme::cloud_muted();
    let heading = text(t!("no_accounts_heading").to_string())
        .size(20)
        .style(theme::colored_text(theme::TEXT_PRIMARY));
    let sub = text(t!("no_accounts_sub").to_string())
        .size(13)
        .style(theme::colored_text(theme::TEXT_SECONDARY));
    let add_btn = button(text(t!("add_account_btn").to_string()).size(13))
        .on_press(Message::NavigateTo(View::AddAccount {
            url_input: String::new(),
            error: None,
        }))
        .padding([9, 20])
        .style(theme::primary_button_style);

    container(
        column![cloud, heading, sub, add_btn]
            .spacing(12)
            .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn account_led_color(account: &AccountView) -> iced::Color {
    if account
        .folders
        .iter()
        .any(|f| matches!(f.status, FolderStatus::Error))
    {
        theme::STATUS_ERROR
    } else if account
        .folders
        .iter()
        .any(|f| matches!(f.status, FolderStatus::Syncing))
    {
        theme::STATUS_SYNCING
    } else if account
        .folders
        .iter()
        .all(|f| matches!(f.status, FolderStatus::Paused))
        && !account.folders.is_empty()
    {
        theme::STATUS_PAUSED
    } else {
        theme::STATUS_OK
    }
}

fn account_section(account: &AccountView) -> Element<'_, Message> {
    let led_color = account_led_color(account);
    let led = container(Space::new(7, 7)).style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(led_color)),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    let name = text(&account.display_name)
        .size(13)
        .style(theme::colored_text(theme::TEXT_PRIMARY));
    let url = text(&account.url)
        .size(11)
        .style(theme::colored_text(theme::TEXT_MUTED));

    let settings_btn = button(
        text("⚙")
            .size(13)
            .style(theme::colored_text(theme::TEXT_MUTED)),
    )
    .on_press(Message::NavigateTo(View::AccountSettings(account.id)))
    .padding([3, 7])
    .style(icon_button_style);

    let header = container(
        row![
            led,
            column![name, url].spacing(1),
            Space::with_width(Length::Fill),
            settings_btn
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([8, 10]),
    )
    .width(Length::Fill)
    .style(section_header_style);

    let mut folders = Column::new().spacing(5);
    if account.folders.is_empty() {
        folders = folders.push(
            container(
                text(t!("no_folders_configured").to_string())
                    .size(12)
                    .style(theme::colored_text(theme::TEXT_MUTED)),
            )
            .padding([10, 14]),
        );
    } else {
        for folder in &account.folders {
            folders = folders.push(folder_row(folder, account.id));
        }
    }

    column![header, folders].spacing(5).into()
}

fn folder_row(folder: &crate::model::FolderView, account_id: uuid::Uuid) -> Element<'_, Message> {
    let color = status_color(&folder.status);
    let led = container(Space::new(6, 6)).style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(color)),
        border: iced::Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    let name = text(&folder.display_name)
        .size(12)
        .style(theme::colored_text(theme::TEXT_PRIMARY));
    let path = text(truncate_path(&folder.local_path, 44))
        .size(11)
        .style(theme::colored_text(theme::TEXT_MUTED));
    let info = column![name, path].spacing(2);

    let folder_id = folder.id;
    let local_path = folder.local_path.clone();

    let open_btn = button(
        text("↗")
            .size(12)
            .style(theme::colored_text(theme::TEXT_MUTED)),
    )
    .on_press(Message::OpenFolder(local_path))
    .padding([2, 5])
    .style(icon_button_style);

    let badge_label = if let FolderStatus::Error = &folder.status {
        let n = folder.errors.len();
        if n == 1 {
            t!("folder_status_error_one").to_string()
        } else {
            t!("folder_status_error_other", count = n).to_string()
        }
    } else {
        match &folder.status {
            FolderStatus::Idle => t!("folder_status_idle").to_string(),
            FolderStatus::Syncing => t!("folder_status_syncing").to_string(),
            FolderStatus::Paused => t!("folder_status_paused").to_string(),
            FolderStatus::Error => unreachable!("handled by outer if-let"),
        }
    };

    let badge_msg: Option<Message> = match &folder.status {
        FolderStatus::Idle => Some(Message::ForceSyncFolder(folder_id)),
        FolderStatus::Syncing => Some(Message::PauseFolder(folder_id)),
        FolderStatus::Paused => Some(Message::ResumeFolder(folder_id)),
        FolderStatus::Error => Some(Message::NavigateTo(View::FolderErrors {
            account_id,
            folder_id,
        })),
    };

    let badge_text = text(badge_label).size(11).style(theme::colored_text(color));

    let badge_inner = container(badge_text)
        .style(status_badge_style(color))
        .padding([3, 9]);

    let badge: Element<Message> = if let Some(msg) = badge_msg {
        button(badge_inner)
            .on_press(msg)
            .padding(0)
            .style(|_theme, _status| button::Style::default())
            .into()
    } else {
        badge_inner.into()
    };

    let progress: Element<Message> = if let Some((done, total)) = folder.progress {
        let pct = if total > 0 {
            done as f32 / total as f32 * 100.0
        } else {
            0.0
        };
        progress_bar(0.0..=100.0, pct).height(3).into()
    } else {
        Space::new(0, 0).into()
    };

    let right = column![
        row![progress, Space::with_width(Length::Fill)].spacing(0),
        row![open_btn, badge].spacing(6).align_y(Alignment::Center),
    ]
    .spacing(4)
    .align_x(Alignment::End);

    container(
        row![led, info, Space::with_width(Length::Fill), right]
            .spacing(10)
            .align_y(Alignment::Center)
            .padding([10, 14]),
    )
    .width(Length::Fill)
    .style(card_style)
    .into()
}

pub fn truncate_path(path: &str, max_chars: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_chars {
        return path.to_string();
    }
    let half = max_chars / 2 - 1;
    let front: String = path.chars().take(half).collect();
    let back: String = path.chars().skip(char_count - half).collect();
    format!("{front}…{back}")
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
        assert!(
            result.chars().count() <= 41,
            "truncated path too long: {result}"
        );
        assert!(
            result.contains('…'),
            "truncated path should contain ellipsis"
        );
    }

    #[test]
    fn non_ascii_path_does_not_panic() {
        let long = "/home/user/Bücher/sehr/tief/verschachtelt/Datei.txt";
        let result = truncate_path(long, 30);
        assert!(result.chars().count() <= 31);
        assert!(result.contains('…'));
    }

    #[test]
    fn cjk_path_does_not_panic() {
        let long = "/home/user/文档/很长的路径/子目录/另一个目录/文件名.txt";
        let result = truncate_path(long, 20);
        assert!(result.chars().count() <= 21);
        assert!(result.contains('…'));
    }
}

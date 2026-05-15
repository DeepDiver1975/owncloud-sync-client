use iced::{
    widget::{button, column, container, text, Column},
    Element, Length,
};
use uuid::Uuid;

use crate::app::Message;
use crate::model::{AccountView, View};
use crate::theme::{self, card_style, ghost_button_style};

pub fn folder_errors_view(account: &AccountView, folder_id: Uuid) -> Element<'_, Message> {
    let folder = account.folders.iter().find(|f| f.id == folder_id);

    let (display_name, local_path, errors): (&str, &str, &[String]) = match folder {
        Some(f) => (&f.display_name, &f.local_path, &f.errors),
        None => return empty_fallback(),
    };

    let name = text(display_name)
        .size(13)
        .style(theme::colored_text(theme::TEXT_PRIMARY));
    let path = text(local_path)
        .size(11)
        .style(theme::colored_text(theme::TEXT_MUTED));
    let header = column![name, path].spacing(2);

    let errors_label = text("SYNC ERRORS")
        .size(10)
        .style(theme::colored_text(theme::TEXT_MUTED));

    let error_list: Element<Message> = if errors.is_empty() {
        container(
            text("No errors recorded.")
                .size(12)
                .style(theme::colored_text(theme::TEXT_MUTED)),
        )
        .padding([10, 14])
        .width(Length::Fill)
        .style(card_style)
        .into()
    } else {
        let mut col = Column::new().spacing(5);
        for err in errors {
            col = col.push(
                container(
                    text(err)
                        .size(12)
                        .style(theme::colored_text(theme::STATUS_ERROR)),
                )
                .padding([10, 14])
                .width(Length::Fill)
                .style(card_style),
            );
        }
        iced::widget::scrollable(col).width(Length::Fill).into()
    };

    let back_btn = button(text("← Back").size(12))
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding([6, 12])
        .style(ghost_button_style);

    let col = column![header, errors_label, error_list, back_btn]
        .spacing(12)
        .padding([16, 20]);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn empty_fallback() -> Element<'static, Message> {
    container(
        text("Folder not found.")
            .size(12)
            .style(theme::colored_text(theme::TEXT_MUTED)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FolderStatus, FolderView};

    fn make_account(folder_errors: Vec<String>) -> (AccountView, Uuid) {
        let folder_id = Uuid::new_v4();
        let account = AccountView {
            id: Uuid::new_v4(),
            url: "https://cloud.example.com".to_string(),
            display_name: "Test Account".to_string(),
            folders: vec![FolderView {
                id: folder_id,
                space_id: String::new(),
                display_name: "Documents".to_string(),
                local_path: "/home/user/docs".to_string(),
                status: FolderStatus::Error,
                progress: None,
                errors: folder_errors,
            }],
        };
        (account, folder_id)
    }

    #[test]
    fn renders_with_errors_without_panic() {
        let (account, folder_id) = make_account(vec![
            "HTTP 503: Service Unavailable".to_string(),
            "I/O error: permission denied".to_string(),
        ]);
        let _el = folder_errors_view(&account, folder_id);
    }

    #[test]
    fn renders_with_empty_errors_without_panic() {
        let (account, folder_id) = make_account(vec![]);
        let _el = folder_errors_view(&account, folder_id);
    }

    #[test]
    fn renders_with_unknown_folder_id_without_panic() {
        let (account, _) = make_account(vec!["some error".to_string()]);
        let unknown_id = Uuid::new_v4();
        let _el = folder_errors_view(&account, unknown_id);
    }
}

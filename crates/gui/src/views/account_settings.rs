use iced::{
    widget::{button, column, container, row, text, Column},
    Element, Length,
};

use crate::app::Message;
use crate::model::{AccountView, View};

const PADDING: u16 = 12;
const SPACING: u16 = 8;

pub fn account_settings_view(account: &AccountView) -> Element<'_, Message> {
    let title = text("Account Settings").size(22);
    let url_label = text(format!("Server: {}", account.url)).size(14);
    let folders_title = text("Synced folders:").size(16);

    let mut folders_col = Column::new().spacing(SPACING / 2);
    if account.folders.is_empty() {
        folders_col = folders_col.push(text("No folders configured.").size(13));
    } else {
        for folder in &account.folders {
            let folder_row = row![
                text(&folder.display_name).size(14),
                text("→").size(14),
                text(&folder.local_path).size(13),
            ]
            .spacing(SPACING / 2);
            folders_col = folders_col.push(folder_row);
        }
    }

    let remove_btn = button("Remove Account")
        .on_press(Message::RemoveAccount(account.id))
        .padding(PADDING);

    let back_btn = button("Back")
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding(PADDING / 2);

    let col = column![
        title,
        url_label,
        folders_title,
        folders_col,
        remove_btn,
        back_btn
    ]
    .spacing(SPACING)
    .max_width(480);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(PADDING * 2)
        .into()
}

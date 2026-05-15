use iced::{
    widget::{button, column, container, row, text, Column},
    Alignment, Element, Length,
};
use rust_i18n::t;

use crate::app::Message;
use crate::model::{AccountView, View};
use crate::theme;

pub fn account_settings_view(account: &AccountView) -> Element<'_, Message> {
    let acct_name = text(&account.display_name)
        .size(15)
        .style(theme::colored_text(theme::TEXT_PRIMARY));
    let acct_url = text(&account.url)
        .size(11)
        .style(theme::colored_text(theme::TEXT_MUTED));

    let remove_btn = button(text(t!("remove_account_btn").to_string()).size(12))
        .on_press(Message::RemoveAccount(account.id))
        .padding([6, 12])
        .style(theme::danger_button_style);

    let add_space_btn = button(text("Add Space…").size(12))
        .on_press(Message::AddSpaceClicked {
            account_id: account.id,
        })
        .padding([6, 12])
        .style(theme::ghost_button_style);

    let header = row![
        column![acct_name, acct_url].spacing(2),
        iced::widget::horizontal_space(),
        add_space_btn,
        remove_btn,
    ]
    .align_y(Alignment::Start)
    .spacing(12);

    let folders_label = text(t!("synced_folders_label").to_string())
        .size(10)
        .style(theme::colored_text(theme::TEXT_MUTED));

    let mut folders_col = Column::new().spacing(0);
    if account.folders.is_empty() {
        folders_col = folders_col.push(
            container(
                text(t!("no_folders_configured_dot").to_string())
                    .size(12)
                    .style(theme::colored_text(theme::TEXT_MUTED)),
            )
            .padding([10, 14]),
        );
    } else {
        for folder in &account.folders {
            let row_widget = container(
                row![
                    text(&folder.display_name)
                        .size(12)
                        .style(theme::colored_text(theme::TEXT_PRIMARY)),
                    text("→")
                        .size(11)
                        .style(theme::colored_text(theme::TEXT_MUTED)),
                    text(&folder.local_path)
                        .size(11)
                        .style(theme::colored_text(theme::TEXT_MUTED)),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding([8, 12]),
            )
            .width(Length::Fill)
            .style(|_: &iced::Theme| iced::widget::container::Style {
                border: iced::Border {
                    color: theme::BORDER_SUBTLE,
                    width: 0.0,
                    ..Default::default()
                },
                ..Default::default()
            });
            folders_col = folders_col.push(row_widget);
        }
    }

    let folders_card = container(folders_col)
        .width(Length::Fill)
        .style(theme::card_style);

    let back_btn = button(text(t!("back_btn").to_string()).size(12))
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding([6, 12])
        .style(theme::ghost_button_style);

    let col = column![header, folders_label, folders_card, back_btn]
        .spacing(12)
        .padding([16, 20]);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

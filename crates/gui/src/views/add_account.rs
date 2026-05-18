use iced::{
    widget::{button, column, container, row, text, text_input},
    Alignment, Element, Length,
};

use rust_i18n::t;

use crate::app::Message;
use crate::model::View;
use crate::theme::{self, t_text};

pub static URL_INPUT_ID: std::sync::LazyLock<text_input::Id> =
    std::sync::LazyLock::new(text_input::Id::unique);

pub fn add_account_view<'a>(url_input: &'a str, error: Option<&'a str>) -> Element<'a, Message> {
    let heading = t_text(t!("add_account_heading"))
        .size(20)
        .style(theme::colored_text(theme::TEXT_PRIMARY));

    let caption = t_text(t!("add_account_caption"))
        .size(13)
        .style(theme::colored_text(theme::TEXT_SECONDARY));

    let url_label = t_text(t!("server_url_label"))
        .size(11)
        .style(theme::colored_text(theme::TEXT_SECONDARY));

    let url_field = text_input("https://your.server.com", url_input)
        .id(URL_INPUT_ID.clone())
        .on_input(Message::AddAccountUrlChanged)
        .on_submit(Message::AddAccountSubmit)
        .padding([9, 11])
        .size(13)
        .style(theme::text_input_style);

    let helper = t_text(t!("server_url_helper"))
        .size(11)
        .style(theme::colored_text(theme::TEXT_MUTED));

    let connect_btn = button(t_text(t!("connect_btn")).size(13))
        .on_press(Message::AddAccountSubmit)
        .padding([9, 18])
        .style(theme::primary_button_style);

    let cancel_btn = button(t_text(t!("cancel_btn")).size(13))
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding([8, 14])
        .style(theme::ghost_button_style);

    let mut col = column![
        heading,
        caption,
        column![url_label, url_field, helper].spacing(4),
    ]
    .spacing(14)
    .max_width(420);

    if let Some(err) = error {
        let banner = container(
            text(err)
                .size(12)
                .style(theme::colored_text(theme::STATUS_ERROR)),
        )
        .style(theme::error_banner_style)
        .padding([8, 12])
        .width(Length::Fill);
        col = col.push(banner);
    }

    col = col.push(
        row![connect_btn, cancel_btn]
            .spacing(10)
            .align_y(Alignment::Center),
    );

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding([24, 28])
        .into()
}

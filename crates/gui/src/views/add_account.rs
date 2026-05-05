use iced::{
    widget::{button, column, container, text, text_input},
    Element, Length,
};

use crate::app::Message;
use crate::model::View;

const PADDING: u16 = 12;
const SPACING: u16 = 8;

pub static URL_INPUT_ID: std::sync::LazyLock<text_input::Id> =
    std::sync::LazyLock::new(text_input::Id::unique);

pub fn add_account_view<'a>(url_input: &'a str, error: Option<&'a str>) -> Element<'a, Message> {
    let title = text("Add ownCloud account").size(22);

    let subtitle = text(
        "Enter your ownCloud server address. You will be redirected \
         to the browser to complete sign-in.",
    )
    .size(14);

    let url_field = text_input("https://your.server.com", url_input)
        .id(URL_INPUT_ID.clone())
        .on_input(Message::AddAccountUrlChanged)
        .on_submit(Message::AddAccountSubmit)
        .padding(PADDING);

    let connect_btn = button("Connect")
        .on_press(Message::AddAccountSubmit)
        .padding(PADDING);

    let back_btn = button("Cancel")
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding(PADDING / 2);

    let mut col = column![title, subtitle, url_field, connect_btn]
        .spacing(SPACING)
        .max_width(480);

    if let Some(err_text) = error {
        col = col.push(text(err_text).size(13));
    }

    col = col.push(back_btn);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(PADDING * 2)
        .into()
}

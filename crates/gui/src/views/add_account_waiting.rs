use iced::{
    widget::{button, column, container, text},
    Element, Length,
};

use crate::app::Message;
use crate::model::View;

const PADDING: u16 = 12;
const SPACING: u16 = 8;

pub fn add_account_waiting_view<'a>() -> Element<'a, Message> {
    let title = text("Waiting for browser sign-in…").size(22);
    let subtitle = text("Complete sign-in in the browser window that just opened.").size(14);

    let cancel_btn = button("Cancel")
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding(PADDING / 2);

    let col = column![title, subtitle, cancel_btn]
        .spacing(SPACING)
        .max_width(480);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(PADDING * 2)
        .into()
}

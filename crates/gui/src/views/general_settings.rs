use iced::{
    widget::{button, column, container, text},
    Element, Length,
};

use crate::app::Message;
use crate::model::View;

const PADDING: u16 = 12;
const SPACING: u16 = 8;

pub fn general_settings_view() -> Element<'static, Message> {
    let title = text("General Settings").size(22);
    let placeholder = text("General settings coming soon.").size(14);

    let back_btn = button("Back")
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding(PADDING / 2);

    let col = column![title, placeholder, back_btn]
        .spacing(SPACING)
        .max_width(480);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(PADDING as u16 * 2)
        .into()
}

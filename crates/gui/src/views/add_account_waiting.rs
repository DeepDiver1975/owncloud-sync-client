use iced::{
    widget::{button, column, container, text},
    Alignment, Element, Length,
};

use crate::app::Message;
use crate::model::View;
use crate::theme;

pub fn add_account_waiting_view<'a>() -> Element<'a, Message> {
    let spinner = text("⟳").size(28).style(theme::colored_text(theme::ACCENT));

    let heading = text("Waiting for sign-in…")
        .size(15)
        .style(theme::colored_text(theme::TEXT_PRIMARY));

    let caption = text("Complete authentication in the browser window that just opened.")
        .size(13)
        .style(theme::colored_text(theme::TEXT_SECONDARY));

    let cancel_btn = button(text("Cancel").size(13))
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding([8, 14])
        .style(theme::ghost_button_style);

    container(
        column![spinner, heading, caption, cancel_btn]
            .spacing(12)
            .align_x(Alignment::Center)
            .max_width(380),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .padding([24, 28])
    .into()
}

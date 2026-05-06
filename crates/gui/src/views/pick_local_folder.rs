use iced::{
    widget::{button, column, container, row, text, text_input},
    Alignment, Element, Length,
};

use crate::app::Message;
use crate::theme;

pub fn pick_local_folder_view<'a>(
    display_name: &'a str,
    url: &'a str,
    local_path_input: &'a str,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let heading = text("Choose a local folder")
        .size(16)
        .style(theme::colored_text(theme::TEXT_PRIMARY));

    let caption = text(format!(
        "Where should {} from {} sync to?",
        display_name, url
    ))
    .size(13)
    .style(theme::colored_text(theme::TEXT_SECONDARY));

    let path_label = text("Local folder path")
        .size(11)
        .style(theme::colored_text(theme::TEXT_SECONDARY));

    let path_field = text_input("~/ownCloud", local_path_input)
        .on_input(Message::PickLocalFolderPathChanged)
        .on_submit(Message::PickLocalFolderSubmit)
        .padding([9, 11])
        .size(13)
        .style(theme::text_input_style);

    let confirm_btn = button(text("Start Syncing").size(13))
        .on_press(Message::PickLocalFolderSubmit)
        .padding([9, 18])
        .style(theme::primary_button_style);

    let cancel_btn = button(text("Cancel").size(13))
        .on_press(Message::PickLocalFolderCancel)
        .padding([8, 14])
        .style(theme::ghost_button_style);

    let mut col = column![heading, caption, column![path_label, path_field].spacing(4),]
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
        row![confirm_btn, cancel_btn]
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

use iced::{
    widget::{button, column, container, text, text_input},
    Element, Length,
};

use crate::app::Message;

pub fn pick_local_folder_view<'a>(
    display_name: &'a str,
    url: &'a str,
    local_path_input: &'a str,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let title = text("Choose sync folder").size(22);
    let account_label = text(format!("Account: {display_name} — {url}")).size(13);
    let instruction = text("Choose a local folder where your files will be synced.").size(14);

    let path_field = text_input("/home/user/ownCloud", local_path_input)
        .on_input(Message::PickLocalFolderPathChanged)
        .on_submit(Message::PickLocalFolderSubmit)
        .padding(12);

    let start_btn = button("Start syncing")
        .on_press(Message::PickLocalFolderSubmit)
        .padding(12);

    let cancel_btn = button("Cancel")
        .on_press(Message::PickLocalFolderCancel)
        .padding(6);

    let mut col = column![title, account_label, instruction, path_field, start_btn]
        .spacing(8)
        .max_width(480);

    if let Some(err_text) = error {
        col = col.push(text(err_text).size(13));
    }

    col = col.push(cancel_btn);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(24)
        .into()
}

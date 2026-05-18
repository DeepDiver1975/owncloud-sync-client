use iced::{
    widget::{button, column, container, row, text, Column},
    Alignment, Element, Length,
};
use rust_i18n::t;

use crate::app::Message;
use crate::model::{SpaceInfo, View};
use crate::theme::{self, t_text};

pub fn pick_root_folder_view<'a>(
    account_id: uuid::Uuid,
    spaces: &'a [SpaceInfo],
    local_path: Option<&'a str>,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let heading = t_text(t!("pick_root_folder_heading"))
        .size(16)
        .style(theme::colored_text(theme::TEXT_PRIMARY));

    let caption = t_text(t!("pick_root_folder_caption"))
        .size(13)
        .style(theme::colored_text(theme::TEXT_SECONDARY));

    let folder_label = t_text(t!("root_folder_label"))
        .size(11)
        .style(theme::colored_text(theme::TEXT_SECONDARY));

    let folder_well = match local_path {
        None => container(
            t_text(t!("no_folder_selected"))
                .size(13)
                .style(theme::colored_text(theme::TEXT_MUTED)),
        )
        .style(theme::folder_well_empty_style)
        .padding([10, 12])
        .width(Length::Fill),
        Some(path) => container(
            row![
                text("📁").size(14),
                text(path)
                    .size(13)
                    .style(theme::colored_text(theme::TEXT_PRIMARY)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .style(theme::folder_well_style)
        .padding([10, 12])
        .width(Length::Fill),
    };

    let mut preview_col = Column::new().spacing(4);
    if let Some(root) = local_path {
        let preview_label = t_text(t!("will_create_label"))
            .size(11)
            .style(theme::colored_text(theme::TEXT_MUTED));
        preview_col = preview_col.push(preview_label);
        for space in spaces {
            let derived = format!("{}/{}", root.trim_end_matches('/'), space.name);
            preview_col = preview_col.push(
                text(derived)
                    .size(11)
                    .style(theme::colored_text(theme::TEXT_SECONDARY)),
            );
        }
    }

    let browse_label = if local_path.is_none() {
        t!("choose_folder_btn").to_string()
    } else {
        t!("change_folder_btn").to_string()
    };
    let browse_btn = button(t_text(browse_label).size(13))
        .on_press(Message::PickRootFolderBrowse)
        .padding([8, 14])
        .style(theme::ghost_button_style);

    let confirm_btn = {
        let b = button(t_text(t!("start_syncing_btn")).size(13))
            .padding([9, 18])
            .style(theme::primary_button_style);
        if local_path.is_some() {
            b.on_press(Message::PickRootFolderSubmit { account_id })
        } else {
            b
        }
    };

    let cancel_btn = button(t_text(t!("cancel_btn")).size(13))
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding([8, 14])
        .style(theme::ghost_button_style);

    let mut col = column![
        heading,
        caption,
        column![folder_label, folder_well, preview_col, browse_btn].spacing(6),
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

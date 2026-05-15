use iced::{
    widget::{button, checkbox, column, container, row, scrollable, text},
    Alignment, Element, Length,
};
use std::collections::HashSet;

use crate::app::Message;
use crate::model::{SpaceInfo, View};
use crate::theme;

pub fn pick_spaces_view<'a>(
    account_id: uuid::Uuid,
    spaces: &'a [SpaceInfo],
    selected: &'a HashSet<String>,
    error: Option<&'a str>,
    title: &'a str,
) -> Element<'a, Message> {
    let heading = text(title)
        .size(16)
        .style(theme::colored_text(theme::TEXT_PRIMARY));

    let caption = text("Select the spaces you want to sync.")
        .size(13)
        .style(theme::colored_text(theme::TEXT_SECONDARY));

    let mut spaces_col = column![].spacing(8);
    for space in spaces {
        let is_checked = selected.contains(&space.id);
        let space_id = space.id.clone();
        let label = format!(
            "{} ({})",
            space.name,
            match space.drive_type.as_str() {
                "personal" => "Personal",
                "project" => "Project",
                _ => "Space",
            }
        );
        let cb = checkbox(label, is_checked)
            .on_toggle(move |checked| Message::ToggleSpaceSelection {
                account_id,
                space_id: space_id.clone(),
                selected: checked,
            })
            .size(14)
            .spacing(8);
        spaces_col = spaces_col.push(cb);
    }

    let next_btn = {
        let b = button(text("Next →").size(13))
            .padding([9, 18])
            .style(theme::primary_button_style);
        if !selected.is_empty() {
            b.on_press(Message::PickSpacesNext { account_id })
        } else {
            b
        }
    };

    let cancel_btn = button(text("Cancel").size(13))
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding([8, 14])
        .style(theme::ghost_button_style);

    let mut col = column![
        heading,
        caption,
        scrollable(spaces_col).height(Length::Fixed(220.0)),
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
        row![next_btn, cancel_btn]
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

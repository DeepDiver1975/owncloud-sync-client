use iced::{
    widget::{button, column, container, row, text, Column},
    Alignment, Element, Length,
};

use crate::app::Message;
use crate::model::View;
use crate::theme;

struct ToggleRow {
    label: &'static str,
    sublabel: &'static str,
    enabled: bool,
}

fn toggle_row(row: &ToggleRow) -> Element<'static, Message> {
    let lbl = text(row.label)
        .size(12)
        .style(theme::colored_text(theme::TEXT_PRIMARY));
    let sub = text(row.sublabel)
        .size(10)
        .style(theme::colored_text(theme::TEXT_MUTED));

    // Visual-only toggle pill (no interaction wired — see spec out-of-scope note)
    let (track_color, thumb_x) = if row.enabled {
        (
            theme::ACCENT,
            iced::Padding {
                top: 2.0,
                right: 2.0,
                bottom: 2.0,
                left: 15.0,
            },
        )
    } else {
        (
            theme::BORDER_DEFAULT,
            iced::Padding {
                top: 2.0,
                right: 15.0,
                bottom: 2.0,
                left: 2.0,
            },
        )
    };

    let thumb = container(iced::widget::Space::new(13, 13)).style(move |_| {
        iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color::WHITE)),
            border: iced::Border {
                radius: 7.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });

    let toggle = container(thumb)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Background::Color(track_color)),
            border: iced::Border {
                radius: 9.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .width(30)
        .height(17)
        .padding(thumb_x);

    container(
        row![
            column![lbl, sub].spacing(2),
            iced::widget::horizontal_space(),
            toggle,
        ]
        .align_y(Alignment::Center)
        .padding([10, 14]),
    )
    .width(Length::Fill)
    .style(|_| iced::widget::container::Style {
        border: iced::Border {
            color: theme::BORDER_SUBTLE,
            width: 0.0,
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

pub fn general_settings_view() -> Element<'static, Message> {
    let heading = text("General Settings")
        .size(15)
        .style(theme::colored_text(theme::TEXT_PRIMARY));

    let rows = [
        ToggleRow {
            label: "Launch at login",
            sublabel: "Start automatically when you log in",
            enabled: true,
        },
        ToggleRow {
            label: "Show in system tray",
            sublabel: "Keep the tray icon visible",
            enabled: true,
        },
        ToggleRow {
            label: "Desktop notifications",
            sublabel: "Notify when syncs complete or fail",
            enabled: false,
        },
    ];

    let mut rows_col = Column::new().spacing(0);
    for r in &rows {
        rows_col = rows_col.push(toggle_row(r));
    }

    let settings_card = container(rows_col)
        .width(Length::Fill)
        .style(theme::card_style);

    let back_btn = button(text("← Back").size(12))
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding([6, 12])
        .style(theme::ghost_button_style);

    let col = column![heading, settings_card, back_btn]
        .spacing(12)
        .padding([16, 20])
        .max_width(480);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

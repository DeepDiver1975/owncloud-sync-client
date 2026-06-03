use iced::{
    widget::{button, column, container, pick_list, row, Column},
    Alignment, Element, Length,
};
use rust_i18n::t;

use crate::app::Message;
use crate::model::{Language, View};
use crate::theme::{self, t_text};

struct ToggleRow {
    label: String,
    sublabel: String,
    enabled: bool,
}

fn toggle_row(row: &ToggleRow) -> Element<'static, Message> {
    let lbl = t_text(row.label.clone())
        .size(12)
        .style(theme::colored_text(theme::TEXT_PRIMARY));
    let sub = t_text(row.sublabel.clone())
        .size(10)
        .style(theme::colored_text(theme::TEXT_MUTED));

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

    let thumb = container(iced::widget::Space::new().width(13).height(13)).style(move |_| {
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
            iced::widget::Space::new().width(Length::Fill),
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

pub fn general_settings_view(language: &Language) -> Element<'_, Message> {
    let heading = t_text(t!("general_settings_heading"))
        .size(15)
        .style(theme::colored_text(theme::TEXT_PRIMARY));

    // Language picker row
    let lang_label = t_text(t!("language_label"))
        .size(12)
        .style(theme::colored_text(theme::TEXT_PRIMARY));
    let lang_sub = t_text(t!("language_sublabel"))
        .size(10)
        .style(theme::colored_text(theme::TEXT_MUTED));

    let lang_picker = pick_list(
        Language::all(),
        Some(language.clone()),
        Message::LanguageChanged,
    )
    .text_size(12)
    .padding([5, 10])
    .text_shaping(iced::widget::text::Shaping::Advanced);

    let lang_row = container(
        row![
            column![lang_label, lang_sub].spacing(2),
            iced::widget::Space::new().width(Length::Fill),
            lang_picker,
        ]
        .align_y(Alignment::Center)
        .padding([10, 14]),
    )
    .width(Length::Fill)
    .style(theme::card_style);

    // Toggle rows
    let rows = [
        ToggleRow {
            label: t!("launch_at_login_label").to_string(),
            sublabel: t!("launch_at_login_sub").to_string(),
            enabled: true,
        },
        ToggleRow {
            label: t!("show_in_tray_label").to_string(),
            sublabel: t!("show_in_tray_sub").to_string(),
            enabled: true,
        },
        ToggleRow {
            label: t!("notifications_label").to_string(),
            sublabel: t!("notifications_sub").to_string(),
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

    let back_btn = button(t_text(t!("back_btn")).size(12))
        .on_press(Message::NavigateTo(View::SyncStatus))
        .padding([6, 12])
        .style(theme::ghost_button_style);

    let col = column![heading, lang_row, settings_card, back_btn]
        .spacing(12)
        .padding([16, 20])
        .max_width(480);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

use iced::{
    widget::{button, column, container, row, text},
    Element, Length,
};
use rust_i18n::t;

use crate::app::Message;
use crate::theme;

include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

pub fn about_view() -> Element<'static, Message> {
    let heading = text(t!("about_heading").to_string())
        .size(15)
        .style(theme::colored_text(theme::TEXT_PRIMARY));

    // Card 1: logo + version/credits/copyright
    let logo = theme::owncloud_icon_large();

    let version_line = row![
        text(format!(
            "Version {}. For more information visit ",
            APP_VERSION
        ))
        .size(11)
        .style(theme::colored_text(theme::TEXT_PRIMARY)),
        button(
            text("https://owncloud.com")
                .size(11)
                .style(theme::colored_text(theme::ACCENT)),
        )
        .on_press(Message::OpenUrl("https://owncloud.com".into()))
        .padding(0)
        .style(theme::link_button_style),
    ]
    .align_y(iced::Alignment::Center);

    let issues_line = row![
        text("For known issues and help, please visit: ") // i18n-ignore
            .size(11)
            .style(theme::colored_text(theme::TEXT_PRIMARY)),
        button(
            text("https://central.owncloud.com")
                .size(11)
                .style(theme::colored_text(theme::ACCENT)),
        )
        .on_press(Message::OpenUrl("https://central.owncloud.com".into()))
        .padding(0)
        .style(theme::link_button_style),
    ]
    .align_y(iced::Alignment::Center);

    let credits = column![
        version_line,
        issues_line,
        text(format!("By {}", CONTRIBUTORS))
            .size(10)
            .style(theme::colored_text(theme::TEXT_MUTED)),
        text("Copyright ownCloud GmbH (A Kiteworks Company)") // i18n-ignore
            .size(10)
            .style(theme::colored_text(theme::TEXT_MUTED)),
        text("Distributed under the GNU General Public License v2") // i18n-ignore
            .size(10)
            .style(theme::colored_text(theme::TEXT_MUTED)),
    ]
    .spacing(4);

    let info_card = container(
        row![logo, credits]
            .spacing(16)
            .align_y(iced::Alignment::Start)
            .padding([14, 16]),
    )
    .width(Length::Fill)
    .style(theme::card_style);

    // Card 2: build info (monospaced)
    let build_lines = column![
        text(format!("ownCloud Sync {}", APP_VERSION))
            .size(10)
            .style(theme::colored_text(theme::TEXT_SECONDARY)),
        text(format!(
            "Libraries: iced {}, rustls {}, libsqlite3-sys {}",
            LIB_ICED, LIB_RUSTLS, LIB_SQLITE
        ))
        .size(10)
        .style(theme::colored_text(theme::TEXT_MUTED)),
        text(format!(
            "OS: {} {} (build arch: {}, CPU arch: {})",
            OS_NAME, OS_VERSION, BUILD_ARCH, CPU_ARCH
        ))
        .size(10)
        .style(theme::colored_text(theme::TEXT_MUTED)),
    ]
    .spacing(3)
    .padding([12, 16]);

    let build_card = container(build_lines)
        .width(Length::Fill)
        .style(theme::card_style);

    let col = column![heading, info_card, build_card]
        .spacing(12)
        .padding([16, 20])
        .max_width(560);

    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

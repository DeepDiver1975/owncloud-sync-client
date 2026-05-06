use iced::{
    widget::{button, container, text, text_input},
    Background, Border, Color,
};

// ---------------------------------------------------------------------------
// Colour palette — dark mode
// ---------------------------------------------------------------------------

pub const BG_BASE: Color = Color {
    r: 0.102,
    g: 0.118,
    b: 0.137,
    a: 1.0,
}; // #1a1e23
pub const BG_SURFACE: Color = Color {
    r: 0.067,
    g: 0.078,
    b: 0.094,
    a: 1.0,
}; // #111418
pub const BG_CARD: Color = Color {
    r: 0.110,
    g: 0.129,
    b: 0.157,
    a: 1.0,
}; // #1c2128
pub const BG_HOVER: Color = Color {
    r: 0.129,
    g: 0.149,
    b: 0.176,
    a: 1.0,
}; // #21262d

pub const BORDER_SUBTLE: Color = Color {
    r: 0.180,
    g: 0.200,
    b: 0.220,
    a: 1.0,
}; // #2e3338
pub const BORDER_DEFAULT: Color = Color {
    r: 0.267,
    g: 0.298,
    b: 0.337,
    a: 1.0,
}; // #444c56

pub const TEXT_PRIMARY: Color = Color {
    r: 0.788,
    g: 0.820,
    b: 0.851,
    a: 1.0,
}; // #c9d1d9
pub const TEXT_SECONDARY: Color = Color {
    r: 0.545,
    g: 0.580,
    b: 0.620,
    a: 1.0,
}; // #8b949e
pub const TEXT_MUTED: Color = Color {
    r: 0.416,
    g: 0.463,
    b: 0.506,
    a: 1.0,
}; // #6e7681

pub const ACCENT: Color = Color {
    r: 0.000,
    g: 0.510,
    b: 0.788,
    a: 1.0,
}; // #0082C9

pub const STATUS_OK: Color = Color {
    r: 0.247,
    g: 0.725,
    b: 0.314,
    a: 1.0,
}; // #3fb950
pub const STATUS_SYNCING: Color = Color {
    r: 0.345,
    g: 0.643,
    b: 0.831,
    a: 1.0,
}; // #58a6d4
pub const STATUS_ERROR: Color = Color {
    r: 0.973,
    g: 0.3176,
    b: 0.286,
    a: 1.0,
}; // #f85149
pub const STATUS_PAUSED: Color = Color {
    r: 0.824,
    g: 0.600,
    b: 0.133,
    a: 1.0,
}; // #d29922

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

pub fn palette() -> iced::theme::Palette {
    iced::theme::Palette {
        background: BG_BASE,
        text: TEXT_PRIMARY,
        primary: ACCENT,
        success: STATUS_OK,
        danger: STATUS_ERROR,
    }
}

pub fn app_theme() -> iced::Theme {
    iced::Theme::custom("OcSync".to_string(), palette())
}

// ---------------------------------------------------------------------------
// Container styles
// ---------------------------------------------------------------------------

pub fn sidebar_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_SURFACE)),
        ..Default::default()
    }
}

pub fn content_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_BASE)),
        ..Default::default()
    }
}

pub fn card_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_CARD)),
        border: Border {
            color: BORDER_SUBTLE,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

pub fn section_header_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG_SURFACE)),
        border: Border {
            color: BORDER_SUBTLE,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

pub fn error_banner_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            r: STATUS_ERROR.r * 0.12,
            g: STATUS_ERROR.g * 0.12,
            b: STATUS_ERROR.b * 0.12,
            a: 1.0,
        })),
        border: Border {
            color: STATUS_ERROR,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

pub fn status_badge_style(color: Color) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.12, ..color })),
        border: Border {
            color: Color { a: 0.30, ..color },
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Button styles
// ---------------------------------------------------------------------------

pub fn primary_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Active => ACCENT,
        button::Status::Hovered => Color {
            r: ACCENT.r + 0.05,
            g: ACCENT.g + 0.04,
            b: ACCENT.b + 0.04,
            a: 1.0,
        },
        button::Status::Pressed => Color {
            r: ACCENT.r * 0.85,
            g: ACCENT.g * 0.85,
            b: ACCENT.b * 0.85,
            a: 1.0,
        },
        button::Status::Disabled => BG_CARD,
    };
    let text_color = if matches!(status, button::Status::Disabled) {
        TEXT_MUTED
    } else {
        Color::WHITE
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn ghost_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let (bg, text_color) = match status {
        button::Status::Active => (None, TEXT_SECONDARY),
        button::Status::Hovered => (Some(Background::Color(BG_HOVER)), TEXT_PRIMARY),
        button::Status::Pressed => (Some(Background::Color(BG_CARD)), TEXT_PRIMARY),
        button::Status::Disabled => (None, TEXT_MUTED),
    };
    button::Style {
        background: bg,
        text_color,
        border: Border {
            color: BORDER_DEFAULT,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

pub fn nav_active_style(_theme: &iced::Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color {
            r: ACCENT.r,
            g: ACCENT.g,
            b: ACCENT.b,
            a: 0.13,
        })),
        text_color: STATUS_SYNCING,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn nav_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let (bg, text_color) = match status {
        button::Status::Active => (None, TEXT_SECONDARY),
        button::Status::Hovered => (Some(Background::Color(BG_HOVER)), TEXT_PRIMARY),
        button::Status::Pressed => (Some(Background::Color(BG_CARD)), TEXT_PRIMARY),
        button::Status::Disabled => (None, TEXT_MUTED),
    };
    button::Style {
        background: bg,
        text_color,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn icon_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let (bg, text_color) = match status {
        button::Status::Active => (None, TEXT_MUTED),
        button::Status::Hovered => (Some(Background::Color(BG_HOVER)), TEXT_SECONDARY),
        button::Status::Pressed => (Some(Background::Color(BG_CARD)), TEXT_PRIMARY),
        button::Status::Disabled => (None, TEXT_MUTED),
    };
    button::Style {
        background: bg,
        text_color,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn danger_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let (bg, text_color, border_color) = match status {
        button::Status::Active => (
            Color {
                r: 0.243,
                g: 0.082,
                b: 0.078,
                a: 1.0,
            },
            STATUS_ERROR,
            STATUS_ERROR,
        ),
        button::Status::Hovered => (
            Color {
                r: 0.350,
                g: 0.110,
                b: 0.104,
                a: 1.0,
            },
            Color::WHITE,
            STATUS_ERROR,
        ),
        button::Status::Pressed => (
            Color {
                r: 0.180,
                g: 0.055,
                b: 0.053,
                a: 1.0,
            },
            Color::WHITE,
            STATUS_ERROR,
        ),
        button::Status::Disabled => (BG_SURFACE, TEXT_MUTED, BORDER_SUBTLE),
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Text input style
// ---------------------------------------------------------------------------

pub fn text_input_style(_theme: &iced::Theme, status: text_input::Status) -> text_input::Style {
    match status {
        text_input::Status::Active => text_input::Style {
            background: Background::Color(BG_CARD),
            border: Border {
                color: BORDER_DEFAULT,
                width: 1.0,
                radius: 6.0.into(),
            },
            icon: TEXT_SECONDARY,
            placeholder: TEXT_MUTED,
            value: TEXT_PRIMARY,
            selection: Color { a: 0.3, ..ACCENT },
        },
        text_input::Status::Focused => text_input::Style {
            background: Background::Color(BG_CARD),
            border: Border {
                color: ACCENT,
                width: 1.5,
                radius: 6.0.into(),
            },
            icon: ACCENT,
            placeholder: TEXT_MUTED,
            value: TEXT_PRIMARY,
            selection: Color { a: 0.3, ..ACCENT },
        },
        text_input::Status::Hovered => text_input::Style {
            background: Background::Color(BG_CARD),
            border: Border {
                color: BORDER_DEFAULT,
                width: 1.0,
                radius: 6.0.into(),
            },
            icon: TEXT_SECONDARY,
            placeholder: TEXT_MUTED,
            value: TEXT_PRIMARY,
            selection: Color { a: 0.3, ..ACCENT },
        },
        text_input::Status::Disabled => text_input::Style {
            background: Background::Color(BG_SURFACE),
            border: Border {
                color: BORDER_SUBTLE,
                width: 1.0,
                radius: 6.0.into(),
            },
            icon: TEXT_MUTED,
            placeholder: TEXT_MUTED,
            value: TEXT_MUTED,
            selection: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        },
    }
}

// ---------------------------------------------------------------------------
// Text style helper
// ---------------------------------------------------------------------------

pub fn colored_text(color: Color) -> impl Fn(&iced::Theme) -> text::Style {
    move |_| text::Style { color: Some(color) }
}

// ---------------------------------------------------------------------------
// Status helpers
// ---------------------------------------------------------------------------

pub fn status_color(status: &crate::model::FolderStatus) -> Color {
    match status {
        crate::model::FolderStatus::Idle => STATUS_OK,
        crate::model::FolderStatus::Syncing => STATUS_SYNCING,
        crate::model::FolderStatus::Error => STATUS_ERROR,
        crate::model::FolderStatus::Paused => STATUS_PAUSED,
    }
}

pub fn status_label(status: &crate::model::FolderStatus) -> &'static str {
    match status {
        crate::model::FolderStatus::Idle => "↻ Sync Now",
        crate::model::FolderStatus::Syncing => "⏸ Pause",
        crate::model::FolderStatus::Error => "⚠ Error",
        crate::model::FolderStatus::Paused => "▶ Resume",
    }
}

// ---------------------------------------------------------------------------
// SVG icon
// ---------------------------------------------------------------------------

pub fn owncloud_icon() -> iced::widget::Svg<'static> {
    let handle = iced::widget::svg::Handle::from_memory(
        include_bytes!("../assets/owncloud-icon.svg").as_slice(),
    );
    iced::widget::Svg::new(handle).width(22).height(22)
}

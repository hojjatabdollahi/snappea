use std::borrow::Cow;

use cosmic::iced::{Color, Length};
use cosmic::widget::{self, Text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppIcon {
    Arrow,
    Circle,
    Square,
    Redact,
    Pixelate,
    Magnifier,
    Timer,
    Ocr,
    Qr,
    Drag,
    Minimize,
    Github,
}

pub fn preload_font() {
    let mut font_system = cosmic::iced::advanced::graphics::text::font_system()
        .write()
        .unwrap();

    font_system.load_font(Cow::Borrowed(crate::lucide_icon::FONT));
}

pub fn icon(icon: AppIcon, size: f32) -> Text<'static, cosmic::Theme, cosmic::Renderer> {
    widget::text(codepoint(icon))
        .font(cosmic::font::Font::with_name("lucide"))
        .size(size)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .center()
}

pub fn icon_with_color(
    icon: AppIcon,
    size: f32,
    color: Color,
) -> Text<'static, cosmic::Theme, cosmic::Renderer> {
    self::icon(icon, size).class(cosmic::theme::Text::Color(color))
}

pub fn icon_with_opacity(
    icon: AppIcon,
    size: f32,
    _opacity: f32,
    _active: bool,
) -> Text<'static, cosmic::Theme, cosmic::Renderer> {
    self::icon(icon, size)
}

fn codepoint(icon: AppIcon) -> &'static str {
    let icon_name = match icon {
        AppIcon::Arrow => "arrow",
        AppIcon::Circle => "circle",
        AppIcon::Square => "square",
        AppIcon::Redact => "redact",
        AppIcon::Pixelate => "pixelate",
        AppIcon::Magnifier => "magnifier",
        AppIcon::Timer => "timer",
        AppIcon::Ocr => "ocr",
        AppIcon::Qr => "qr",
        AppIcon::Drag => "drag",
        AppIcon::Minimize => "minimize",
        AppIcon::Github => "github",
    };

    crate::lucide_icon::ALL_ICONS
        .iter()
        .find_map(|(name, codepoint)| (*name == icon_name).then_some(*codepoint))
        .expect("Lucide icon must exist in generated icon map")
}

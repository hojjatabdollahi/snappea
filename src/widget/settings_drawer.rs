//! Settings drawer widget that opens relative to the toolbar

use cosmic::iced::Length;
use cosmic::iced_core::Border;
use cosmic::iced_widget::{column, row, toggler};
use cosmic::widget::{container, text};
use cosmic::Element;

use super::toolbar::HoverOpacity;
use crate::screenshot::ToolbarPosition;

/// Build the settings drawer element
pub fn build_settings_drawer<'a, Msg: Clone + 'static>(
    _toolbar_position: ToolbarPosition,
    magnifier_enabled: bool,
    on_magnifier_toggle: Msg,
    space_s: u16,
    _space_xs: u16,
) -> Element<'a, Msg> {
    // Build the content of the settings drawer
    let magnifier_row = row![
        text::body("Magnifier"),
        cosmic::widget::horizontal_space(),
        toggler(magnifier_enabled)
            .on_toggle(move |_| on_magnifier_toggle.clone())
            .size(24.0),
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

    let drawer_content: Element<'_, Msg> = column![magnifier_row,]
        .spacing(space_s)
        .padding(space_s)
        .width(Length::Fixed(200.0))
        .into();

    // Wrap in a styled container with transparent background
    // HoverOpacity will handle the background drawing with opacity
    let drawer = container(drawer_content).class(cosmic::theme::Container::Custom(Box::new(
        |theme| {
            let cosmic_theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: None, // HoverOpacity handles the background
                text_color: Some(cosmic_theme.background.component.on.into()),
                border: Border::default(),
                ..Default::default()
            }
        },
    )));

    // Wrap in HoverOpacity - always opaque since drawer is only visible when open
    HoverOpacity::new(drawer).force_opaque(true).into()
}


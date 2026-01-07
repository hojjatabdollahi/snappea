//! Settings drawer widget that opens relative to the toolbar

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::iced_core::Border;
use cosmic::iced_widget::{column, row, toggler};
use cosmic::widget::{container, radio, text};

use super::toolbar::HoverOpacity;
use crate::config::{SaveLocation, ToolbarPosition};

/// Build the settings drawer element
#[allow(clippy::too_many_arguments)]
pub fn build_settings_drawer<'a, Msg: Clone + 'static>(
    _toolbar_position: ToolbarPosition,
    magnifier_enabled: bool,
    on_magnifier_toggle: Msg,
    save_location: SaveLocation,
    on_save_location_pictures: Msg,
    on_save_location_documents: Msg,
    copy_to_clipboard_on_save: bool,
    on_copy_on_save_toggle: Msg,
    space_s: u16,
    space_xs: u16,
) -> Element<'a, Msg> {
    // Magnifier toggle row
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

    // Save location section
    let save_location_label = text::body("Save to:");

    let pictures_radio = radio(
        "Pictures",
        SaveLocation::Pictures,
        Some(save_location),
        move |_| on_save_location_pictures.clone(),
    );

    let documents_radio = radio(
        "Documents",
        SaveLocation::Documents,
        Some(save_location),
        move |_| on_save_location_documents.clone(),
    );

    let save_location_row = row![pictures_radio, documents_radio,]
        .spacing(space_s)
        .align_y(cosmic::iced_core::Alignment::Center);

    // Copy to clipboard on save toggle
    let copy_on_save_row = row![
        text::body("Copy on save"),
        cosmic::widget::horizontal_space(),
        toggler(copy_to_clipboard_on_save)
            .on_toggle(move |_| on_copy_on_save_toggle.clone())
            .size(24.0),
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

    let drawer_content: Element<'_, Msg> = column![
        magnifier_row,
        cosmic::widget::divider::horizontal::light(),
        save_location_label,
        save_location_row,
        cosmic::widget::divider::horizontal::light(),
        copy_on_save_row,
    ]
    .spacing(space_xs)
    .padding(space_s)
    .width(Length::Fixed(220.0))
    .into();

    // Wrap in a styled container with transparent background
    // HoverOpacity will handle the background drawing with opacity
    let drawer =
        container(drawer_content).class(cosmic::theme::Container::Custom(Box::new(|theme| {
            let cosmic_theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: None, // HoverOpacity handles the background
                text_color: Some(cosmic_theme.background.component.on.into()),
                border: Border::default(),
                ..Default::default()
            }
        })));

    // Wrap in HoverOpacity - always opaque since drawer is only visible when open
    HoverOpacity::new(drawer).force_opaque(true).into()
}

//! Settings drawer widget that opens relative to the toolbar

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::iced_core::Border;
use cosmic::iced_widget::{column, row, toggler};
use cosmic::widget::{container, dropdown, radio, segmented_button, tab_bar, text};

use super::toolbar::HoverOpacity;
use crate::config::{Container, SaveLocation, ToolbarPosition};
use crate::fl;
use crate::session::state::SettingsTab;

/// Available framerate options
const FRAMERATE_OPTIONS: &[u32] = &[24, 30, 60];

/// All container format options
const CONTAINER_OPTIONS: &[Container] = &[Container::Mp4, Container::Webm, Container::Mkv];

/// Build the settings drawer element
#[allow(clippy::too_many_arguments)]
pub fn build_settings_drawer<'a, Msg: Clone + 'static, F, G, H>(
    _toolbar_position: ToolbarPosition,
    magnifier_enabled: bool,
    on_magnifier_toggle: Msg,
    save_location: SaveLocation,
    on_save_location_pictures: Msg,
    on_save_location_documents: Msg,
    copy_to_clipboard_on_save: bool,
    on_copy_on_save_toggle: Msg,
    on_github_click: Msg,
    settings_tab: SettingsTab,
    settings_tab_model: &'a segmented_button::SingleSelectModel,
    on_settings_tab_activate: impl Fn(segmented_button::Entity) -> Msg + 'static,
    toolbar_unhovered_opacity: f32,
    on_toolbar_opacity_change: impl Fn(f32) -> Msg + Clone + 'a,
    // Recording settings
    available_encoders: Vec<(String, String)>, // (display_name, gst_element)
    selected_encoder: Option<String>,
    on_encoder_select: F,
    video_container: Container,
    on_container_select: G,
    video_framerate: u32,
    on_framerate_select: H,
    video_show_cursor: bool,
    on_show_cursor_toggle: Msg,
    hide_toolbar_to_tray: bool,
    on_hide_to_tray_toggle: Msg,
    space_s: u16,
    space_xs: u16,
) -> Element<'a, Msg>
where
    F: Fn(String) -> Msg + Clone + Send + Sync + 'static,
    G: Fn(Container) -> Msg + Clone + Send + Sync + 'static,
    H: Fn(u32) -> Msg + Clone + Send + Sync + 'static,
{
    // Build tab row using tab_bar style (looks like tabs instead of segmented control)
    // The callback receives the Entity, and the handler will look up the SettingsTab data
    let tabs_row: Element<'_, Msg> = tab_bar::horizontal(settings_tab_model)
        .button_height(32)
        .on_activate(on_settings_tab_activate)
        .into();

    // Magnifier toggle row
    let magnifier_row = row![
        text::body(fl!("magnifier")),
        cosmic::widget::horizontal_space(),
        toggler(magnifier_enabled)
            .on_toggle(move |_| on_magnifier_toggle.clone())
            .size(24.0),
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

    // Save location section
    let save_location_label = text::body(fl!("save-location"));

    let pictures_radio = radio(
        text::body(fl!("pictures")),
        SaveLocation::Pictures,
        Some(save_location),
        move |_| on_save_location_pictures.clone(),
    );

    let documents_radio = radio(
        text::body(fl!("documents")),
        SaveLocation::Documents,
        Some(save_location),
        move |_| on_save_location_documents.clone(),
    );

    let save_location_row = row![pictures_radio, documents_radio,]
        .spacing(space_s)
        .align_y(cosmic::iced_core::Alignment::Center);

    // Copy to clipboard on save toggle
    let copy_on_save_row = row![
        text::body(fl!("copy-on-save")),
        cosmic::widget::horizontal_space(),
        toggler(copy_to_clipboard_on_save)
            .on_toggle(move |_| on_copy_on_save_toggle.clone())
            .size(24.0),
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

    let opacity_percent = (toolbar_unhovered_opacity.clamp(0.1, 1.0) * 100.0).round() as i32;
    let toolbar_opacity_label = text::body(fl!("toolbar-opacity", percent = opacity_percent));
    let toolbar_opacity_slider = cosmic::widget::slider(20..=100, opacity_percent, move |v| {
        on_toolbar_opacity_change(v as f32 / 100.0)
    })
    .step(5)
    .width(Length::Fill);
    let toolbar_opacity_section = column![toolbar_opacity_label, toolbar_opacity_slider]
        .spacing(space_xs)
        .width(Length::Fill);

    // Encoder selection using dropdown
    let encoder_label = text::body(fl!("encoder"));

    // Find selected encoder index
    let selected_encoder_idx = available_encoders
        .iter()
        .position(|(_, element)| Some(element.as_str()) == selected_encoder.as_deref());

    // Create list of display names for the dropdown (leak to get 'static lifetime)
    let encoder_display_names: &'static [String] = Box::leak(
        available_encoders
            .iter()
            .map(|(display, _)| display.clone())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );

    // Clone the encoder elements for the callback
    let encoder_elements: Vec<String> = available_encoders
        .iter()
        .map(|(_, element)| element.clone())
        .collect();

    let encoder_dropdown = dropdown(encoder_display_names, selected_encoder_idx, move |idx| {
        let element = encoder_elements.get(idx).cloned().unwrap_or_default();
        on_encoder_select(element)
    })
    .width(Length::Fill);

    // Container format selection using dropdown
    let container_label = text::body(fl!("format"));

    // Use static array for container names (these are technical names, not translated)
    static CONTAINER_NAMES: &[&str] = &["MP4", "WebM", "MKV"];

    let selected_container_idx = CONTAINER_OPTIONS.iter().position(|c| *c == video_container);

    let container_dropdown = dropdown(CONTAINER_NAMES, selected_container_idx, move |idx| {
        let container = CONTAINER_OPTIONS
            .get(idx)
            .copied()
            .unwrap_or(Container::Mp4);
        on_container_select(container)
    })
    .width(Length::Fill);

    // Framerate selection using dropdown
    let framerate_label = text::body(fl!("framerate"));

    // Use static array for framerate names
    static FRAMERATE_NAMES: &[&str] = &["24 fps", "30 fps", "60 fps"];

    let selected_framerate_idx = FRAMERATE_OPTIONS
        .iter()
        .position(|fps| *fps == video_framerate);

    let framerate_dropdown = dropdown(FRAMERATE_NAMES, selected_framerate_idx, move |idx| {
        let fps = FRAMERATE_OPTIONS.get(idx).copied().unwrap_or(30);
        on_framerate_select(fps)
    })
    .width(Length::Fill);

    // Show cursor toggle
    let show_cursor_row = row![
        text::caption(fl!("show-cursor")),
        cosmic::widget::horizontal_space(),
        toggler(video_show_cursor)
            .on_toggle(move |_| on_show_cursor_toggle.clone())
            .size(20.0),
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

    // Hide to system tray toggle
    let hide_to_tray_row = row![
        text::caption(fl!("hide-to-tray")),
        cosmic::widget::horizontal_space(),
        toggler(hide_toolbar_to_tray)
            .on_toggle(move |_| on_hide_to_tray_toggle.clone())
            .size(20.0),
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

    // About section
    const SNAPPEA_LOGO: &[u8] = include_bytes!("../../data/logo.svg");
    const GITHUB_ICON: &[u8] =
        include_bytes!("../../data/icons/hicolor/scalable/actions/github.svg");

    let version = env!("CARGO_PKG_VERSION");

    let snappea_logo =
        cosmic::widget::icon(cosmic::widget::icon::from_svg_bytes(SNAPPEA_LOGO).symbolic(false))
            .width(Length::Fixed(32.0))
            .height(Length::Fixed(32.0));

    let about_section = row![
        snappea_logo,
        column![
            row![
                text::title4(fl!("app-name")),
                text::caption(format!("v{}", version)),
            ]
            .spacing(space_xs)
            .align_y(cosmic::iced_core::Alignment::Center),
            row![
                text::caption(fl!("app-author")),
                cosmic::widget::button::icon(
                    cosmic::widget::icon::from_svg_bytes(GITHUB_ICON).symbolic(true)
                )
                .extra_small()
                .on_press(on_github_click),
            ]
            .spacing(space_xs)
            .align_y(cosmic::iced_core::Alignment::Center),
        ]
        .spacing(space_xs)
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center);

    let picture_tab_content: Element<'_, Msg> = column![
        save_location_label,
        save_location_row,
        cosmic::widget::divider::horizontal::light(),
        copy_on_save_row,
    ]
    .spacing(space_xs)
    .into();

    // Build rows with label and dropdown side by side
    let encoder_row = row![
        encoder_label,
        cosmic::widget::horizontal_space(),
        encoder_dropdown,
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

    let container_row = row![
        container_label,
        cosmic::widget::horizontal_space(),
        container_dropdown,
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

    let framerate_row = row![
        framerate_label,
        cosmic::widget::horizontal_space(),
        framerate_dropdown,
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

    let video_tab_content: Element<'_, Msg> = column![
        encoder_row,
        cosmic::widget::divider::horizontal::light(),
        container_row,
        cosmic::widget::divider::horizontal::light(),
        framerate_row,
        cosmic::widget::divider::horizontal::light(),
        show_cursor_row,
        cosmic::widget::divider::horizontal::light(),
        hide_to_tray_row,
    ]
    .spacing(space_xs)
    .into();

    let general_tab_content: Element<'_, Msg> = column![
        magnifier_row,
        cosmic::widget::divider::horizontal::light(),
        toolbar_opacity_section,
        cosmic::widget::divider::horizontal::light(),
        about_section,
    ]
    .spacing(space_xs)
    .into();

    let tab_content: Element<'_, Msg> = match settings_tab {
        SettingsTab::General => general_tab_content,
        SettingsTab::Picture => picture_tab_content,
        SettingsTab::Video => video_tab_content,
    };

    let drawer_content: Element<'_, Msg> = column![tabs_row, tab_content,]
        .spacing(space_xs)
        .padding(space_s)
        .width(Length::Fixed(280.0))
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

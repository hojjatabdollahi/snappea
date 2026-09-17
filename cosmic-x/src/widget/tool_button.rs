// SPDX-License-Identifier: GPL-3.0-only

//! Popups for the shape and pencil tools.

use cosmic::Element;
use cosmic::iced::core::{Background, Border, Length};
use cosmic::iced::widget::{column, row};
use cosmic::widget::{button, container, icon, text, tooltip};

use crate::config::ShapeTool;
use crate::config::{SHAPE_THICKNESS_MAX, SHAPE_THICKNESS_MIN};
use crate::fl;
use viewer_tools::annotate::AnnotateColor;

/// Get color name from i18n for a given index
pub fn color_name(index: usize) -> String {
    match index {
        0 => fl!("color-white"),
        1 => fl!("color-red"),
        2 => fl!("color-orange"),
        3 => fl!("color-green"),
        4 => fl!("color-blue"),
        5 => fl!("color-black"),
        _ => String::new(),
    }
}

/// Width of the shape popup: fits the tool rows and four swatches per row.
const SHAPE_POPUP_WIDTH: f32 = 260.0;

/// Build the shape settings popup element
#[allow(clippy::too_many_arguments)]
pub fn build_shape_popup<'a, Msg: Clone + 'static>(
    current_tool: ShapeTool,
    current_color: AnnotateColor,
    has_annotations: bool,
    on_select_tool: &(impl Fn(ShapeTool) -> Msg + 'a),
    current_thickness: f32,
    on_thickness_change: impl Fn(f32) -> Msg + 'a,
    on_thickness_save: Msg,
    on_color_change: &(impl Fn(AnnotateColor) -> Msg + 'a),
    on_clear: Msg,
    space_s: u16,
    space_xs: u16,
) -> Element<'a, Msg> {
    // Tool list: one row per tool (icon + label + selected check).
    //
    let tool_rows: Vec<Element<'_, Msg>> = ShapeTool::ALL
        .iter()
        .map(|tool| {
            let tool = *tool;
            let is_selected = tool == current_tool;
            let item = row![
                icon::Icon::from(icon::from_name(tool.icon_name()).size(16)),
                text::body(tool.label()).width(Length::Fill),
                if is_selected {
                    Element::from(icon::Icon::from(
                        icon::from_name("object-select-symbolic").size(16),
                    ))
                } else {
                    Element::from(cosmic::iced::widget::space().width(Length::Fixed(16.0)))
                },
            ]
            .spacing(space_xs)
            .align_y(cosmic::iced::core::Alignment::Center)
            .width(Length::Fill);

            button::custom(item)
                .class(if is_selected {
                    cosmic::theme::Button::Suggested
                } else {
                    cosmic::theme::Button::Icon
                })
                .width(Length::Fill)
                .padding(space_xs)
                .on_press(on_select_tool(tool))
                .into()
        })
        .collect();

    let shape_buttons = cosmic::iced::widget::Column::with_children(tool_rows)
        .spacing(2)
        .width(Length::Fill);

    // Subtitle with keyboard shortcuts
    let shape_subtitle = container(text::caption(fl!("shape-cycle-hint")).class(
        cosmic::theme::Text::Color(cosmic::iced::Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
    ))
    .width(Length::Fill)
    .align_x(cosmic::iced::core::alignment::Horizontal::Center);

    // Stroke thickness
    let thickness_section: Element<'_, Msg> = {
        column![
            text::caption(fl!("thickness", size = (current_thickness as u32))),
            cosmic::widget::slider(
                SHAPE_THICKNESS_MIN..=SHAPE_THICKNESS_MAX,
                current_thickness,
                on_thickness_change,
            )
            .step(1.0_f32)
            .on_release(on_thickness_save)
            .width(Length::Fill),
            cosmic::widget::divider::horizontal::light(),
        ]
        .spacing(space_xs)
        .width(Length::Fill)
        .into()
    };

    // Color picker: 2 rows of 4 color swatches each to avoid clipping
    let make_color_swatch = |color: &AnnotateColor, name: String| {
        let is_selected = (color.0.r - current_color.0.r).abs() < 0.05
            && (color.0.g - current_color.0.g).abs() < 0.05
            && (color.0.b - current_color.0.b).abs() < 0.05;
        let color_val = *color;
        let iced_color: cosmic::iced::Color = color_val.into();

        tooltip(
            button::custom(
                container(cosmic::iced::widget::space().width(Length::Fixed(0.0)))
                    .width(Length::Fixed(24.0))
                    .height(Length::Fixed(24.0))
                    .class(cosmic::theme::Container::Custom(Box::new(move |_theme| {
                        cosmic::iced::widget::container::Style {
                            background: Some(Background::Color(iced_color)),
                            border: Border {
                                radius: 4.0.into(),
                                width: if is_selected { 2.0 } else { 1.0 },
                                color: if is_selected {
                                    cosmic::iced::Color::WHITE
                                } else {
                                    cosmic::iced::Color::from_rgba(0.5, 0.5, 0.5, 0.5)
                                },
                            },
                            ..Default::default()
                        }
                    }))),
            )
            .class(cosmic::theme::Button::Text)
            .on_press(on_color_change(color_val))
            .padding(2),
            text::body(name),
            tooltip::Position::Bottom,
        )
    };

    // Split across two rows, however many presets there are.
    let half = AnnotateColor::PRESETS.len().div_ceil(2);
    let swatch_row = |range: std::ops::Range<usize>| {
        let mut r = row![]
            .spacing(space_xs)
            .align_y(cosmic::iced::core::Alignment::Center);
        for i in range {
            r = r.push(make_color_swatch(&AnnotateColor::PRESETS[i], color_name(i)));
        }
        r
    };
    let color_row1 = swatch_row(0..half);
    let color_row2 = swatch_row(half..AnnotateColor::PRESETS.len());

    // Center the color rows
    let color_row1_centered = container(color_row1)
        .width(Length::Fill)
        .align_x(cosmic::iced::core::alignment::Horizontal::Center);

    let color_row2_centered = container(color_row2)
        .width(Length::Fill)
        .align_x(cosmic::iced::core::alignment::Horizontal::Center);

    let color_section = column![
        text::body(fl!("color")),
        color_row1_centered,
        color_row2_centered
    ]
    .spacing(space_xs)
    .align_x(cosmic::iced::core::Alignment::Start);

    // Shadow toggle
    // Clear button (full width)
    let clear_button = button::custom(
        container(
            row![
                icon::Icon::from(icon::from_name("edit-delete-symbolic").size(16))
                    .width(Length::Fixed(16.0))
                    .height(Length::Fixed(16.0)),
                text::body(fl!("clear-annotations")),
            ]
            .spacing(space_xs)
            .align_y(cosmic::iced::core::Alignment::Center),
        )
        .width(Length::Fill)
        .align_x(cosmic::iced::core::alignment::Horizontal::Center),
    )
    .class(cosmic::theme::Button::Destructive)
    .on_press_maybe(has_annotations.then_some(on_clear))
    .padding([space_xs, space_s])
    .width(Length::Fill);

    let clear_row = container(clear_button).width(Length::Fill);

    let popup_content = column![
        shape_buttons,
        shape_subtitle,
        cosmic::widget::divider::horizontal::light(),
        thickness_section,
        color_section,
        cosmic::widget::divider::horizontal::light(),
        cosmic::widget::divider::horizontal::light(),
        clear_row,
    ]
    .spacing(space_s)
    .padding(space_s)
    .width(Length::Fixed(SHAPE_POPUP_WIDTH));

    container(popup_content)
        .class(cosmic::theme::Container::Custom(Box::new(|theme| {
            let cosmic_theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(
                    cosmic_theme.background(false).component.base.into(),
                )),
                text_color: Some(cosmic_theme.background(false).component.on.into()),
                border: Border {
                    radius: cosmic_theme.corner_radii.radius_s.into(),
                    width: 1.0,
                    color: cosmic::iced::Color::from_rgba(0.5, 0.5, 0.5, 0.3),
                },
                ..Default::default()
            }
        })))
        .into()
}

/// Build the pencil settings popup element for recording annotations
#[allow(clippy::too_many_arguments)]
pub fn build_pencil_popup<'a, Msg: Clone + 'static>(
    current_color: AnnotateColor,
    fade_duration: f32,
    thickness: f32,
    has_annotations: bool,
    on_color_change: &(impl Fn(AnnotateColor) -> Msg + 'a),
    on_duration_change: impl Fn(f32) -> Msg + 'a,
    on_duration_save: Msg,
    on_thickness_change: impl Fn(f32) -> Msg + 'a,
    on_thickness_save: Msg,
    on_clear: Msg,
    space_s: u16,
    space_xs: u16,
) -> Element<'a, Msg> {
    // Color picker: 2 rows of 4 color swatches each
    let make_color_swatch = |color: &AnnotateColor, name: String| {
        let is_selected = (color.0.r - current_color.0.r).abs() < 0.05
            && (color.0.g - current_color.0.g).abs() < 0.05
            && (color.0.b - current_color.0.b).abs() < 0.05;
        let color_val = *color;
        let iced_color: cosmic::iced::Color = color_val.into();

        tooltip(
            button::custom(
                container(cosmic::iced::widget::space().width(Length::Fixed(0.0)))
                    .width(Length::Fixed(24.0))
                    .height(Length::Fixed(24.0))
                    .class(cosmic::theme::Container::Custom(Box::new(move |_theme| {
                        cosmic::iced::widget::container::Style {
                            background: Some(Background::Color(iced_color)),
                            border: Border {
                                radius: 4.0.into(),
                                width: if is_selected { 2.0 } else { 1.0 },
                                color: if is_selected {
                                    cosmic::iced::Color::WHITE
                                } else {
                                    cosmic::iced::Color::from_rgba(0.5, 0.5, 0.5, 0.5)
                                },
                            },
                            ..Default::default()
                        }
                    }))),
            )
            .class(cosmic::theme::Button::Text)
            .on_press(on_color_change(color_val))
            .padding(2),
            text::body(name),
            tooltip::Position::Bottom,
        )
    };

    // Split across two rows, however many presets there are.
    let half = AnnotateColor::PRESETS.len().div_ceil(2);
    let swatch_row = |range: std::ops::Range<usize>| {
        let mut r = row![]
            .spacing(space_xs)
            .align_y(cosmic::iced::core::Alignment::Center);
        for i in range {
            r = r.push(make_color_swatch(&AnnotateColor::PRESETS[i], color_name(i)));
        }
        r
    };
    let color_row1 = swatch_row(0..half);
    let color_row2 = swatch_row(half..AnnotateColor::PRESETS.len());

    // Center the color rows
    let color_row1_centered = container(color_row1)
        .width(Length::Fill)
        .align_x(cosmic::iced::core::alignment::Horizontal::Center);

    let color_row2_centered = container(color_row2)
        .width(Length::Fill)
        .align_x(cosmic::iced::core::alignment::Horizontal::Center);

    let color_section = column![
        text::body(fl!("color")),
        color_row1_centered,
        color_row2_centered
    ]
    .spacing(space_xs)
    .align_x(cosmic::iced::core::Alignment::Start);

    // Thickness slider (1-10 pixels): updates during drag, saves on release
    let thickness_label = text::body(fl!("thickness", size = (thickness as u32)));
    let thickness_slider = cosmic::widget::slider(1.0..=10.0, thickness, on_thickness_change)
        .step(1.0_f32)
        .on_release(on_thickness_save)
        .width(Length::Fill);

    let thickness_section = column![thickness_label, thickness_slider,]
        .spacing(space_xs)
        .width(Length::Fill);

    // Fade duration slider (1-10 seconds): updates during drag, saves on release
    let duration_label = text::body(fl!("fade-duration", duration = (fade_duration as u32)));
    let duration_slider = cosmic::widget::slider(1.0..=10.0, fade_duration, on_duration_change)
        .step(1.0_f32)
        .on_release(on_duration_save)
        .width(Length::Fill);

    let duration_section = column![duration_label, duration_slider,]
        .spacing(space_xs)
        .width(Length::Fill);

    // Clear button (full width)
    let clear_button = button::custom(
        container(
            row![
                icon::Icon::from(icon::from_name("edit-delete-symbolic").size(16))
                    .width(Length::Fixed(16.0))
                    .height(Length::Fixed(16.0)),
                text::body(fl!("clear-drawings")),
            ]
            .spacing(space_xs)
            .align_y(cosmic::iced::core::Alignment::Center),
        )
        .width(Length::Fill)
        .align_x(cosmic::iced::core::alignment::Horizontal::Center),
    )
    .class(cosmic::theme::Button::Destructive)
    .on_press_maybe(has_annotations.then_some(on_clear))
    .padding([space_xs, space_s])
    .width(Length::Fill);

    let clear_row = container(clear_button).width(Length::Fill);

    let popup_content = column![
        color_section,
        cosmic::widget::divider::horizontal::light(),
        thickness_section,
        cosmic::widget::divider::horizontal::light(),
        duration_section,
        cosmic::widget::divider::horizontal::light(),
        clear_row,
    ]
    .spacing(space_s)
    .padding(space_s)
    .width(Length::Fixed(230.0));

    container(popup_content)
        .class(cosmic::theme::Container::Custom(Box::new(|theme| {
            let cosmic_theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(
                    cosmic_theme.background(false).component.base.into(),
                )),
                text_color: Some(cosmic_theme.background(false).component.on.into()),
                border: Border {
                    radius: cosmic_theme.corner_radii.radius_s.into(),
                    width: 1.0,
                    color: cosmic::iced::Color::from_rgba(0.5, 0.5, 0.5, 0.3),
                },
                ..Default::default()
            }
        })))
        .into()
}

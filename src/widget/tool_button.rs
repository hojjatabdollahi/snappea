//! Generic tool button widget with indicator dots and context popup support
//!
//! This module provides a reusable button widget that:
//! - Displays an icon with indicator dots below showing which option is selected
//! - Normal click (left button) triggers primary action (e.g., toggle mode)
//! - Right-click or long-press triggers secondary action (e.g., open settings popup)
//!
//! Used for shape tools, and can be reused for other multi-option tool buttons.

use cosmic::Element;
use cosmic::iced::Size;
use cosmic::iced_core::{
    Background, Border, Layout, Length, Rectangle, layout, mouse, widget::Tree,
};
use cosmic::iced_widget::{column, row};
use cosmic::widget::{button, container, icon, text, toggler, tooltip};

use crate::config::{RedactTool, ShapeColor, ShapeTool};
use crate::fl;

/// A wrapper widget that detects right-click and long-press events
pub struct RightClickWrapper<'a, Msg> {
    content: Element<'a, Msg>,
    on_right_click: Option<Msg>,
    press_start: std::cell::Cell<Option<std::time::Instant>>,
}

impl<'a, Msg: Clone + 'static> RightClickWrapper<'a, Msg> {
    pub fn new(content: impl Into<Element<'a, Msg>>, on_right_click: Option<Msg>) -> Self {
        Self {
            content: content.into(),
            on_right_click,
            press_start: std::cell::Cell::new(None),
        }
    }
}

impl<'a, Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for RightClickWrapper<'a, Msg>
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &cosmic::iced::Limits,
    ) -> layout::Node {
        self.content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: cosmic::iced_core::Event,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn cosmic::iced_core::Clipboard,
        shell: &mut cosmic::iced_core::Shell<'_, Msg>,
        viewport: &Rectangle,
    ) -> cosmic::iced_core::event::Status {
        // Check for right-click
        if let cosmic::iced_core::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) =
            &event
            && let Some(pos) = cursor.position()
            && layout.bounds().contains(pos)
            && let Some(ref msg) = self.on_right_click
        {
            shell.publish(msg.clone());
            return cosmic::iced_core::event::Status::Captured;
        }

        // Track press start for long-press detection
        if let cosmic::iced_core::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) =
            &event
            && let Some(pos) = cursor.position()
            && layout.bounds().contains(pos)
        {
            self.press_start.set(Some(std::time::Instant::now()));
        }

        // Check for long-press on release (500ms threshold)
        if let cosmic::iced_core::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) =
            &event
            && let Some(start) = self.press_start.take()
            && start.elapsed() >= std::time::Duration::from_millis(500)
            && let Some(pos) = cursor.position()
            && layout.bounds().contains(pos)
            && let Some(ref msg) = self.on_right_click
        {
            shell.publish(msg.clone());
            return cosmic::iced_core::event::Status::Captured;
        }

        // Pass event to content
        self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &Rectangle,
        renderer: &cosmic::Renderer,
    ) -> cosmic::iced_core::mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn cosmic::iced_core::widget::Operation<()>,
    ) {
        self.content
            .as_widget()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        translation: cosmic::iced::Vector,
    ) -> Option<cosmic::iced_core::overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, translation)
    }
}

impl<'a, Msg: Clone + 'static> From<RightClickWrapper<'a, Msg>> for Element<'a, Msg> {
    fn from(wrapper: RightClickWrapper<'a, Msg>) -> Self {
        Element::new(wrapper)
    }
}

/// Preset colors for the color picker (color values only, names come from i18n)
pub const COLOR_PRESETS: &[ShapeColor] = &[
    ShapeColor {
        r: 0.9,
        g: 0.1,
        b: 0.1,
    }, // Red
    ShapeColor {
        r: 0.1,
        g: 0.7,
        b: 0.1,
    }, // Green
    ShapeColor {
        r: 0.1,
        g: 0.4,
        b: 0.9,
    }, // Blue
    ShapeColor {
        r: 0.9,
        g: 0.7,
        b: 0.1,
    }, // Yellow
    ShapeColor {
        r: 0.9,
        g: 0.5,
        b: 0.1,
    }, // Orange
    ShapeColor {
        r: 0.7,
        g: 0.1,
        b: 0.7,
    }, // Purple
    ShapeColor {
        r: 1.0,
        g: 1.0,
        b: 1.0,
    }, // White
    ShapeColor {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    }, // Black
];

/// Get color name from i18n for a given index
pub fn color_name(index: usize) -> String {
    match index {
        0 => fl!("color-red"),
        1 => fl!("color-green"),
        2 => fl!("color-blue"),
        3 => fl!("color-yellow"),
        4 => fl!("color-orange"),
        5 => fl!("color-purple"),
        6 => fl!("color-white"),
        7 => fl!("color-black"),
        _ => String::new(),
    }
}

/// Build a generic tool button with indicator dots.
///
/// This is a reusable button widget that can be used for any multi-option tool.
/// - Normal click (left button): triggers `on_press`
/// - Right click or long press: triggers `on_right_click`
///
/// # Arguments
/// * `icon_name` - The icon to display on the button
/// * `tooltip_text` - Tooltip text shown on hover
/// * `num_options` - Number of indicator dots to show
/// * `current_option_index` - Which dot should be highlighted (0-based)
/// * `is_active` - Whether the tool is currently active
/// * `is_popup_open` - Whether the settings popup is open
/// * `is_enabled` - Whether the button is enabled
/// * `on_press` - Callback for normal click
/// * `on_right_click` - Callback for right-click/long-press
/// * `padding` - Button padding
#[allow(clippy::too_many_arguments)]
pub fn build_tool_button<'a, Msg: Clone + 'static>(
    icon_name: &'a str,
    tooltip_text: impl AsRef<str>,
    num_options: usize,
    current_option_index: usize,
    is_active: bool,
    is_popup_open: bool,
    is_enabled: bool,
    on_press: Option<Msg>,
    on_right_click: Option<Msg>,
    padding: u16,
    content_opacity: f32,
) -> Element<'a, Msg> {
    use std::rc::Rc;

    let main_size = 40.0;
    let dot_size = 6.0_f32;
    let dot_spacing = 4.0_f32;

    let button_class = if is_active || is_popup_open {
        cosmic::theme::Button::Suggested
    } else {
        cosmic::theme::Button::Icon
    };

    // Create icon using iced's Svg with native opacity support
    let icon_svg_handle = icon::Icon::from(icon::from_name(icon_name).size(64))
        .into_svg_handle()
        .expect("Icon should be SVG");

    // Standard button like all other toolbar buttons
    let main_button = if is_enabled {
        button::custom(
            cosmic::iced_widget::svg::Svg::new(icon_svg_handle.clone())
                .width(Length::Fixed(main_size))
                .height(Length::Fixed(main_size))
                .opacity(content_opacity)
                .symbolic(true),
        )
        .class(button_class)
        .on_press_maybe(on_press.clone())
        .padding(padding)
    } else {
        button::custom(
            cosmic::iced_widget::svg::Svg::new(icon_svg_handle.clone())
                .width(Length::Fixed(main_size))
                .height(Length::Fixed(main_size))
                .opacity(content_opacity)
                .symbolic(true),
        )
        .class(button_class)
        .padding(padding)
    };

    // Wrap button in a right-click handler
    let main_button_with_events = RightClickWrapper::new(main_button, on_right_click);

    // Create indicator dots using theme accent color with opacity
    let dot_inactive_color = cosmic::iced::Color::from_rgba(0.5, 0.5, 0.5, 0.5 * content_opacity);

    let make_dot = move |index: usize| {
        let is_active_dot = index == current_option_index;
        let opacity = content_opacity;
        container(cosmic::widget::horizontal_space().width(Length::Fixed(0.0)))
            .width(Length::Fixed(dot_size))
            .height(Length::Fixed(dot_size))
            .class(cosmic::theme::Container::Custom(Box::new(move |theme| {
                let cosmic_theme = theme.cosmic();
                let mut accent_color: cosmic::iced::Color = cosmic_theme.accent_color().into();
                accent_color.a *= opacity;
                cosmic::iced::widget::container::Style {
                    background: Some(Background::Color(if is_active_dot {
                        accent_color
                    } else {
                        dot_inactive_color
                    })),
                    border: Border {
                        radius: (dot_size / 2.0).into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })))
    };

    // Build dots row dynamically based on num_options
    let mut dots: Vec<Element<'_, Msg>> = Vec::with_capacity(num_options);
    for i in 0..num_options {
        dots.push(make_dot(i).into());
    }
    let dots_row = row(dots)
        .spacing(dot_spacing as u16)
        .align_y(cosmic::iced_core::Alignment::Center);

    // Wrap dots in a container for centering
    let dots_container = container(dots_row)
        .width(Length::Fixed(main_size + (padding as f32 * 2.0)))
        .height(Length::Fixed(dot_size))
        .align_x(cosmic::iced_core::alignment::Horizontal::Center);

    // Stack button (with right-click wrapper) and dots vertically
    let combined = column![Element::from(main_button_with_events), dots_container]
        .spacing(2)
        .align_x(cosmic::iced_core::Alignment::Center);

    // Wrap in a container that aligns the top of the button with other buttons
    let aligned_container = container(combined)
        .align_x(cosmic::iced_core::alignment::Horizontal::Center)
        .align_y(cosmic::iced_core::alignment::Vertical::Top);

    let tooltip_string = tooltip_text.as_ref().to_string();
    tooltip(
        aligned_container,
        text::body(tooltip_string),
        tooltip::Position::Bottom,
    )
    .into()
}

/// Build the shape tool button (convenience wrapper around `build_tool_button`).
#[allow(clippy::too_many_arguments)]
pub fn build_shape_button<'a, Msg: Clone + 'static>(
    current_tool: ShapeTool,
    is_active: bool,
    is_popup_open: bool,
    is_enabled: bool,
    on_press: Option<Msg>,
    on_right_click: Option<Msg>,
    padding: u16,
    _space_xxs: u16,
    content_opacity: f32,
) -> Element<'a, Msg> {
    let option_index = match current_tool {
        ShapeTool::Arrow => 0,
        ShapeTool::Circle => 1,
        ShapeTool::Rectangle => 2,
    };

    build_tool_button(
        current_tool.icon_name(),
        current_tool.tooltip(),
        3, // 3 shape options
        option_index,
        is_active,
        is_popup_open,
        is_enabled,
        on_press,
        on_right_click,
        padding,
        content_opacity,
    )
}

/// Build the shape settings popup element
#[allow(clippy::too_many_arguments)]
pub fn build_shape_popup<'a, Msg: Clone + 'static>(
    current_tool: ShapeTool,
    current_color: ShapeColor,
    shadow_enabled: bool,
    has_annotations: bool,
    on_select_arrow: Msg,
    on_select_circle: Msg,
    on_select_rectangle: Msg,
    on_color_change: &(impl Fn(ShapeColor) -> Msg + 'a),
    on_shadow_toggle: Msg,
    on_clear: Msg,
    space_s: u16,
    space_xs: u16,
) -> Element<'a, Msg> {
    let icon_size = 32.0;

    // Shape selection buttons in a row
    let btn_arrow = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("arrow-symbolic").size(48))
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size)),
        )
        .class(if current_tool == ShapeTool::Arrow {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(on_select_arrow)
        .padding(space_xs),
        text::body(fl!("arrow")),
        tooltip::Position::Bottom,
    );

    let btn_circle = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("circle-symbolic").size(48))
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size)),
        )
        .class(if current_tool == ShapeTool::Circle {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(on_select_circle)
        .padding(space_xs),
        text::body(fl!("oval-circle")),
        tooltip::Position::Bottom,
    );

    let btn_rectangle = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("square-symbolic").size(48))
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size)),
        )
        .class(if current_tool == ShapeTool::Rectangle {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(on_select_rectangle)
        .padding(space_xs),
        text::body(fl!("rectangle-square")),
        tooltip::Position::Bottom,
    );

    // Center the shape buttons in the popup
    let shape_buttons = container(
        row![btn_arrow, btn_circle, btn_rectangle]
            .spacing(space_xs)
            .align_y(cosmic::iced_core::Alignment::Center),
    )
    .width(Length::Fill)
    .align_x(cosmic::iced_core::alignment::Horizontal::Center);

    // Subtitle with keyboard shortcuts
    let shape_subtitle = container(text::caption(fl!("shape-cycle-hint")).class(
        cosmic::theme::Text::Color(cosmic::iced::Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
    ))
    .width(Length::Fill)
    .align_x(cosmic::iced_core::alignment::Horizontal::Center);

    // Color picker - 2 rows of 4 color swatches each to avoid clipping
    let make_color_swatch = |color: &ShapeColor, name: String| {
        let is_selected = (color.r - current_color.r).abs() < 0.05
            && (color.g - current_color.g).abs() < 0.05
            && (color.b - current_color.b).abs() < 0.05;
        let color_val = *color;
        let iced_color: cosmic::iced::Color = color_val.into();

        tooltip(
            button::custom(
                container(cosmic::widget::horizontal_space().width(Length::Fixed(0.0)))
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

    // First row: Red, Green, Blue, Yellow
    let color_row1 = row![
        make_color_swatch(&COLOR_PRESETS[0], color_name(0)),
        make_color_swatch(&COLOR_PRESETS[1], color_name(1)),
        make_color_swatch(&COLOR_PRESETS[2], color_name(2)),
        make_color_swatch(&COLOR_PRESETS[3], color_name(3)),
    ]
    .spacing(space_xs)
    .align_y(cosmic::iced_core::Alignment::Center);

    // Second row: Orange, Purple, White, Black
    let color_row2 = row![
        make_color_swatch(&COLOR_PRESETS[4], color_name(4)),
        make_color_swatch(&COLOR_PRESETS[5], color_name(5)),
        make_color_swatch(&COLOR_PRESETS[6], color_name(6)),
        make_color_swatch(&COLOR_PRESETS[7], color_name(7)),
    ]
    .spacing(space_xs)
    .align_y(cosmic::iced_core::Alignment::Center);

    // Center the color rows
    let color_row1_centered = container(color_row1)
        .width(Length::Fill)
        .align_x(cosmic::iced_core::alignment::Horizontal::Center);

    let color_row2_centered = container(color_row2)
        .width(Length::Fill)
        .align_x(cosmic::iced_core::alignment::Horizontal::Center);

    let color_section = column![
        text::body(fl!("color")),
        color_row1_centered,
        color_row2_centered
    ]
    .spacing(space_xs)
    .align_x(cosmic::iced_core::Alignment::Start);

    // Shadow toggle
    let shadow_row = row![
        text::body(fl!("shadow")),
        cosmic::widget::horizontal_space(),
        toggler(shadow_enabled)
            .on_toggle(move |_| on_shadow_toggle.clone())
            .size(20.0),
    ]
    .spacing(space_s)
    .align_y(cosmic::iced_core::Alignment::Center)
    .width(Length::Fill);

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
            .align_y(cosmic::iced_core::Alignment::Center),
        )
        .width(Length::Fill)
        .align_x(cosmic::iced_core::alignment::Horizontal::Center),
    )
    .class(cosmic::theme::Button::Destructive)
    .on_press_maybe(has_annotations.then_some(on_clear))
    .padding([space_xs, space_s])
    .width(Length::Fill);

    let clear_row = container(clear_button).width(Length::Fill);

    // Assemble popup content
    // Width needs to fit: 4 color swatches per row * (24px + 2*2 padding + spacing) + popup padding
    let popup_content = column![
        shape_buttons,
        shape_subtitle,
        cosmic::widget::divider::horizontal::light(),
        color_section,
        cosmic::widget::divider::horizontal::light(),
        shadow_row,
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
                    cosmic_theme.background.component.base.into(),
                )),
                text_color: Some(cosmic_theme.background.component.on.into()),
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

/// Build the redact/pixelate tool popup
#[allow(clippy::too_many_arguments)]
pub fn build_redact_popup<'a, Msg: Clone + 'static>(
    current_tool: RedactTool,
    has_redactions: bool,
    pixelation_block_size: u32,
    on_select_redact: Msg,
    on_select_pixelate: Msg,
    on_set_pixelation_size: impl Fn(u32) -> Msg + 'a,
    on_save_pixelation_size: Msg,
    on_clear: Msg,
    space_s: u16,
    space_xs: u16,
) -> Element<'a, Msg> {
    let icon_size = 32.0;

    // Tool selection buttons in a row
    let btn_redact = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("redact-symbolic").size(48))
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size)),
        )
        .class(if current_tool == RedactTool::Redact {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(on_select_redact)
        .padding(space_xs),
        text::body(fl!("redact-blackout")),
        tooltip::Position::Bottom,
    );

    let btn_pixelate = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("pixelate-symbolic").size(48))
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size)),
        )
        .class(if current_tool == RedactTool::Pixelate {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(on_select_pixelate)
        .padding(space_xs),
        text::body(fl!("pixelate-blur")),
        tooltip::Position::Bottom,
    );

    // Center the tool buttons in the popup
    let tool_buttons = container(
        row![btn_redact, btn_pixelate]
            .spacing(space_xs)
            .align_y(cosmic::iced_core::Alignment::Center),
    )
    .width(Length::Fill)
    .align_x(cosmic::iced_core::alignment::Horizontal::Center);

    // Subtitle with keyboard shortcuts
    let redact_subtitle = container(text::caption(fl!("redact-cycle-hint")).class(
        cosmic::theme::Text::Color(cosmic::iced::Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
    ))
    .width(Length::Fill)
    .align_x(cosmic::iced_core::alignment::Horizontal::Center);

    // Pixelation size slider
    let pixelation_label = text::body(fl!("pixelation-size", size = pixelation_block_size));
    let pixelation_slider =
        cosmic::widget::slider(4..=64, pixelation_block_size as i32, move |v| {
            on_set_pixelation_size(v as u32)
        })
        .step(4i32)
        .on_release(on_save_pixelation_size)
        .width(Length::Fill);

    let pixelation_section = column![pixelation_label, pixelation_slider,]
        .spacing(space_xs)
        .width(Length::Fill);

    // Clear button (full width)
    let clear_button = button::custom(
        container(
            row![
                icon::Icon::from(icon::from_name("edit-delete-symbolic").size(16))
                    .width(Length::Fixed(16.0))
                    .height(Length::Fixed(16.0)),
                text::body(fl!("clear-redactions")),
            ]
            .spacing(space_xs)
            .align_y(cosmic::iced_core::Alignment::Center),
        )
        .width(Length::Fill)
        .align_x(cosmic::iced_core::alignment::Horizontal::Center),
    )
    .class(cosmic::theme::Button::Destructive)
    .on_press_maybe(has_redactions.then_some(on_clear))
    .padding([space_xs, space_s])
    .width(Length::Fill);

    let clear_row = container(clear_button).width(Length::Fill);

    // Assemble popup content
    let popup_content = column![
        tool_buttons,
        redact_subtitle,
        cosmic::widget::divider::horizontal::light(),
        pixelation_section,
        cosmic::widget::divider::horizontal::light(),
        clear_row,
    ]
    .spacing(space_s)
    .padding(space_s)
    .width(Length::Fixed(230.0)); // Same width as shapes popup

    container(popup_content)
        .class(cosmic::theme::Container::Custom(Box::new(|theme| {
            let cosmic_theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(
                    cosmic_theme.background.component.base.into(),
                )),
                text_color: Some(cosmic_theme.background.component.on.into()),
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
    current_color: ShapeColor,
    fade_duration: f32,
    thickness: f32,
    has_annotations: bool,
    on_color_change: &(impl Fn(ShapeColor) -> Msg + 'a),
    on_duration_change: impl Fn(f32) -> Msg + 'a,
    on_duration_save: Msg,
    on_thickness_change: impl Fn(f32) -> Msg + 'a,
    on_thickness_save: Msg,
    on_clear: Msg,
    space_s: u16,
    space_xs: u16,
) -> Element<'a, Msg> {
    // Color picker - 2 rows of 4 color swatches each
    let make_color_swatch = |color: &ShapeColor, name: String| {
        let is_selected = (color.r - current_color.r).abs() < 0.05
            && (color.g - current_color.g).abs() < 0.05
            && (color.b - current_color.b).abs() < 0.05;
        let color_val = *color;
        let iced_color: cosmic::iced::Color = color_val.into();

        tooltip(
            button::custom(
                container(cosmic::widget::horizontal_space().width(Length::Fixed(0.0)))
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

    // First row: Red, Green, Blue, Yellow
    let color_row1 = row![
        make_color_swatch(&COLOR_PRESETS[0], color_name(0)),
        make_color_swatch(&COLOR_PRESETS[1], color_name(1)),
        make_color_swatch(&COLOR_PRESETS[2], color_name(2)),
        make_color_swatch(&COLOR_PRESETS[3], color_name(3)),
    ]
    .spacing(space_xs)
    .align_y(cosmic::iced_core::Alignment::Center);

    // Second row: Orange, Purple, White, Black
    let color_row2 = row![
        make_color_swatch(&COLOR_PRESETS[4], color_name(4)),
        make_color_swatch(&COLOR_PRESETS[5], color_name(5)),
        make_color_swatch(&COLOR_PRESETS[6], color_name(6)),
        make_color_swatch(&COLOR_PRESETS[7], color_name(7)),
    ]
    .spacing(space_xs)
    .align_y(cosmic::iced_core::Alignment::Center);

    // Center the color rows
    let color_row1_centered = container(color_row1)
        .width(Length::Fill)
        .align_x(cosmic::iced_core::alignment::Horizontal::Center);

    let color_row2_centered = container(color_row2)
        .width(Length::Fill)
        .align_x(cosmic::iced_core::alignment::Horizontal::Center);

    let color_section = column![
        text::body(fl!("color")),
        color_row1_centered,
        color_row2_centered
    ]
    .spacing(space_xs)
    .align_x(cosmic::iced_core::Alignment::Start);

    // Thickness slider (1-10 pixels) - updates during drag, saves on release
    let thickness_label = text::body(fl!("thickness", size = (thickness as u32)));
    let thickness_slider =
        cosmic::widget::slider(1.0..=10.0, thickness, move |v| on_thickness_change(v))
            .step(1.0)
            .on_release(on_thickness_save)
            .width(Length::Fill);

    let thickness_section = column![thickness_label, thickness_slider,]
        .spacing(space_xs)
        .width(Length::Fill);

    // Fade duration slider (1-10 seconds) - updates during drag, saves on release
    let duration_label = text::body(fl!("fade-duration", duration = (fade_duration as u32)));
    let duration_slider =
        cosmic::widget::slider(1.0..=10.0, fade_duration, move |v| on_duration_change(v))
            .step(1.0)
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
            .align_y(cosmic::iced_core::Alignment::Center),
        )
        .width(Length::Fill)
        .align_x(cosmic::iced_core::alignment::Horizontal::Center),
    )
    .class(cosmic::theme::Button::Destructive)
    .on_press_maybe(has_annotations.then_some(on_clear))
    .padding([space_xs, space_s])
    .width(Length::Fill);

    let clear_row = container(clear_button).width(Length::Fill);

    // Assemble popup content
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
                    cosmic_theme.background.component.base.into(),
                )),
                text_color: Some(cosmic_theme.background.component.on.into()),
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

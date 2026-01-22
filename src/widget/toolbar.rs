//! Toolbar widget for screenshot actions

use std::rc::Rc;

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::iced_core::{Background, Border, Layout, Size, layout, widget::Tree};
use cosmic::iced_widget::{column, row};
use cosmic::widget::{button, icon, tooltip};

use super::tool_button::{build_shape_button, build_tool_button};
use super::toolbar_position_selector::ToolbarPositionSelector;
use crate::capture::qr::DetectedQrCode;
use crate::config::{RedactTool, ShapeTool, ToolbarPosition};
use crate::domain::{Choice, DragState, Rect};

/// A wrapper widget that reduces opacity when not hovered
/// Draws a background with opacity and passes through all events
/// Used by both toolbar and settings drawer for consistent appearance
pub struct HoverOpacity<'a, Msg> {
    content: Element<'a, Msg>,
    unhovered_opacity: f32,
    /// When true, always use full opacity (ignores hover state)
    force_opaque: bool,
}

impl<'a, Msg: 'static + Clone> HoverOpacity<'a, Msg> {
    pub fn new(content: impl Into<Element<'a, Msg>>) -> Self {
        Self {
            content: content.into(),
            unhovered_opacity: 0.5,
            force_opaque: false,
        }
    }

    /// Force full opacity regardless of hover state
    pub fn force_opaque(mut self, force: bool) -> Self {
        self.force_opaque = force;
        self
    }
}

impl<'a, Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for HoverOpacity<'a, Msg>
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
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

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
    ) {
        use cosmic::iced_core::Renderer as _;

        let bounds = layout.bounds();
        let is_hovered = cursor
            .position()
            .map(|p| bounds.contains(p))
            .unwrap_or(false);
        let opacity = if self.force_opaque || is_hovered {
            1.0
        } else {
            self.unhovered_opacity
        };

        let cosmic_theme = theme.cosmic();
        let radius = cosmic_theme.corner_radii.radius_s;

        // Draw the background with appropriate opacity
        let mut bg_color: cosmic::iced::Color = cosmic_theme.background.component.base.into();
        bg_color.a *= opacity;

        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds,
                border: Border {
                    radius: radius.into(),
                    ..Default::default()
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(bg_color),
        );

        // Apply opacity to the text color style
        let mut draw_style = *style;
        draw_style.text_color.a *= opacity;

        // Draw content
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            &draw_style,
            layout,
            cursor,
            viewport,
        );
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

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: cosmic::iced_core::Event,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn cosmic::iced_core::Clipboard,
        shell: &mut cosmic::iced_core::Shell<'_, Msg>,
        viewport: &cosmic::iced_core::Rectangle,
    ) -> cosmic::iced_core::event::Status {
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
        viewport: &cosmic::iced_core::Rectangle,
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

impl<'a, Msg: Clone + 'static> From<HoverOpacity<'a, Msg>> for Element<'a, Msg> {
    fn from(widget: HoverOpacity<'a, Msg>) -> Self {
        Element::new(widget)
    }
}

/// Build the screenshot toolbar element
#[allow(clippy::too_many_arguments)]
pub fn build_toolbar<'a, Msg: Clone + 'static>(
    choice: Choice,
    output_name: String,
    toolbar_position: ToolbarPosition,
    has_selection: bool,
    has_ocr_text: bool,
    qr_codes: &[DetectedQrCode],
    primary_shape_tool: ShapeTool,
    shape_mode_active: bool,
    shape_popup_open: bool,
    primary_redact_tool: RedactTool,
    redact_mode_active: bool,
    redact_popup_open: bool,
    space_s: u16,
    space_xs: u16,
    space_xxs: u16,
    on_choice_change: impl Fn(Choice) -> Msg + 'static + Clone,
    on_copy_to_clipboard: Msg,
    on_save_to_pictures: Msg,
    on_record_region: Msg,
    on_shape_press: Msg,
    on_shape_right_click: Msg,
    on_redact_press: Msg,
    on_redact_right_click: Msg,
    on_ocr: Msg,
    on_ocr_copy: Msg,
    on_qr: Msg,
    on_qr_copy: Msg,
    on_cancel: Msg,
    on_toolbar_position: &(impl Fn(ToolbarPosition) -> Msg + 'a),
    on_settings_toggle: Msg,
    settings_drawer_open: bool,
    force_toolbar_opaque: bool,
    output_count: usize,
    tesseract_available: bool,
) -> Element<'a, Msg> {
    use cosmic::widget::divider::vertical;

    let is_vertical = matches!(
        toolbar_position,
        ToolbarPosition::Left | ToolbarPosition::Right
    );

    let active_icon =
        cosmic::theme::Svg::Custom(Rc::new(|theme| cosmic::iced_widget::svg::Style {
            color: Some(theme.cosmic().accent_color().into()),
        }));

    // Position selector - custom widget with triangular hit regions
    let position_selector: Element<'_, Msg> = tooltip(
        ToolbarPositionSelector::new(
            40.0, // size of the selector widget
            toolbar_position,
            on_toolbar_position(ToolbarPosition::Top),
            on_toolbar_position(ToolbarPosition::Bottom),
            on_toolbar_position(ToolbarPosition::Left),
            on_toolbar_position(ToolbarPosition::Right),
        ),
        "Move Toolbar (Ctrl+hjkl)",
        tooltip::Position::Bottom,
    )
    .into();

    // Common buttons with tooltips
    let btn_region = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("screenshot-selection-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
                .class(if matches!(choice, Choice::Rectangle(..)) {
                    active_icon.clone()
                } else {
                    cosmic::theme::Svg::default()
                }),
        )
        .selected(matches!(choice, Choice::Rectangle(..)))
        .class(cosmic::theme::Button::Icon)
        .on_press(on_choice_change(Choice::Rectangle(
            Rect::default(),
            DragState::None,
        )))
        .padding(space_xs),
        "Select Region (R)",
        tooltip::Position::Bottom,
    );

    let btn_window = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("screenshot-window-symbolic").size(64))
                .class(if matches!(choice, Choice::Window(..)) {
                    active_icon.clone()
                } else {
                    cosmic::theme::Svg::default()
                })
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .selected(matches!(choice, Choice::Window(..)))
        .class(cosmic::theme::Button::Icon)
        .on_press(on_choice_change(Choice::Window(output_name.clone(), None)))
        .padding(space_xs),
        "Select Window (W)",
        tooltip::Position::Bottom,
    );

    let btn_screen = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("screenshot-screen-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
                .class(if matches!(choice, Choice::Output(..)) {
                    active_icon.clone()
                } else {
                    cosmic::theme::Svg::default()
                }),
        )
        .selected(matches!(choice, Choice::Output(..)))
        .class(cosmic::theme::Button::Icon)
        .on_press(on_choice_change(Choice::Output(None))) // Goes to picker mode
        .padding(space_xs),
        "Select Screen (S)",
        tooltip::Position::Bottom,
    );

    // Context-sensitive copy tooltip
    let copy_tooltip = match &choice {
        Choice::Rectangle(r, _) if r.dimensions().is_some() => "Copy Selected Region (Enter)",
        Choice::Window(_, Some(_)) => "Copy Selected Window (Enter)",
        Choice::Output(Some(_)) => "Copy Selected Screen (Enter)",
        _ if output_count > 1 => "Copy All Screens (Enter)",
        _ => "Copy Screen (Enter)",
    };

    // Context-sensitive save tooltip
    let save_tooltip = match &choice {
        Choice::Rectangle(r, _) if r.dimensions().is_some() => "Save Selected Region (Ctrl+Enter)",
        Choice::Window(_, Some(_)) => "Save Selected Window (Ctrl+Enter)",
        Choice::Output(Some(_)) => "Save Selected Screen (Ctrl+Enter)",
        _ if output_count > 1 => "Save All Screens (Ctrl+Enter)",
        _ => "Save Screen (Ctrl+Enter)",
    };

    // Copy to clipboard button - always enabled
    let btn_copy = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .class(cosmic::theme::Button::Icon)
        .on_press(on_copy_to_clipboard)
        .padding(space_xs),
        copy_tooltip,
        tooltip::Position::Bottom,
    );

    // Save to pictures button - always enabled
    let btn_save = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("document-save-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .class(cosmic::theme::Button::Icon)
        .on_press(on_save_to_pictures)
        .padding(space_xs),
        save_tooltip,
        tooltip::Position::Bottom,
    );

    // Record button - enabled only when region is selected
    let btn_record = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("media-record-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .class(cosmic::theme::Button::Icon)
        .on_press_maybe(has_selection.then_some(on_record_region))
        .padding(space_xs),
        "Record selected region (Shift+R)",
        tooltip::Position::Bottom,
    );

    // Shape drawing button with indicator dots
    // - Normal click: triggers primary action (toggles mode)
    // - Right-click or long-press: triggers secondary action (opens popup)
    let btn_shapes: Element<'_, Msg> = build_shape_button(
        primary_shape_tool,
        shape_mode_active,
        shape_popup_open,
        has_selection,
        has_selection.then_some(on_shape_press.clone()),
        has_selection.then_some(on_shape_right_click.clone()),
        space_xs,
        space_xxs,
    );

    // Redact/Pixelate tool button (combined)
    let btn_redact = build_tool_button(
        primary_redact_tool.icon_name(),
        primary_redact_tool.tooltip(),
        2, // 2 options: Redact and Pixelate
        primary_redact_tool.index(),
        redact_mode_active,
        redact_popup_open,
        has_selection,
        has_selection.then_some(on_redact_press.clone()),
        has_selection.then_some(on_redact_right_click.clone()),
        space_xs,
    );

    // OCR button
    let btn_ocr = if has_ocr_text {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Suggested)
            .on_press_maybe(has_selection.then_some(on_ocr_copy.clone()))
            .padding(space_xs),
            "Copy OCR Text (O)",
            tooltip::Position::Bottom,
        )
    } else if tesseract_available {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("ocr-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Icon)
            .on_press_maybe(has_selection.then_some(on_ocr.clone()))
            .padding(space_xs),
            "Recognize Text (O)",
            tooltip::Position::Bottom,
        )
    } else {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("ocr-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Icon)
            .on_press_maybe(None)
            .padding(space_xs),
            "Install tesseract to enable OCR",
            tooltip::Position::Bottom,
        )
    };

    // QR button
    let has_qr_codes = !qr_codes.is_empty();
    let btn_qr = if has_qr_codes {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Suggested)
            .on_press_maybe(has_selection.then_some(on_qr_copy.clone()))
            .padding(space_xs),
            "Copy QR Code (Q)",
            tooltip::Position::Bottom,
        )
    } else {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("qr-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Icon)
            .on_press_maybe(has_selection.then_some(on_qr.clone()))
            .padding(space_xs),
            "Scan QR Code (Q)",
            tooltip::Position::Bottom,
        )
    };

    // Settings button - responds to both left and right click
    let btn_settings: Element<'_, Msg> = {
        let settings_btn = tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("application-menu-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(if settings_drawer_open {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Icon
            })
            .on_press(on_settings_toggle.clone())
            .padding(space_xs),
            "Settings",
            tooltip::Position::Bottom,
        );
        super::tool_button::RightClickWrapper::new(settings_btn, Some(on_settings_toggle)).into()
    };

    let btn_close = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("window-close-symbolic").size(63))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .class(cosmic::theme::Button::Icon)
        .on_press(on_cancel),
        "Cancel",
        tooltip::Position::Bottom,
    );

    let toolbar_content: Element<'_, Msg> = if is_vertical {
        // Vertical layout for left/right positions
        use cosmic::widget::divider::horizontal;
        if has_selection {
            let tool_buttons = column![btn_shapes, btn_redact, btn_ocr, btn_qr]
                .spacing(space_s)
                .align_x(cosmic::iced_core::Alignment::Center);

            column![
                position_selector,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_region, btn_window, btn_screen]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                tool_buttons,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_copy, btn_save, btn_record]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
            ]
            .align_x(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_s, space_xxs, space_s, space_xxs])
            .into()
        } else {
            column![
                position_selector,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_region, btn_window, btn_screen]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_copy, btn_save, btn_record]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
            ]
            .align_x(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_s, space_xxs, space_s, space_xxs])
            .into()
        }
    } else {
        // Horizontal layout for top/bottom positions
        if has_selection {
            let tool_buttons = row![btn_shapes, btn_redact, btn_ocr, btn_qr]
                .spacing(space_s)
                .align_y(cosmic::iced_core::Alignment::Center);

            row![
                position_selector,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_region, btn_window, btn_screen]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                tool_buttons,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_copy, btn_save, btn_record]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
            ]
            .align_y(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_xxs, space_s, space_xxs, space_s])
            .into()
        } else {
            row![
                position_selector,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_region, btn_window, btn_screen]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_copy, btn_save, btn_record]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
            ]
            .align_y(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_xxs, space_s, space_xxs, space_s])
            .into()
        }
    };

    // Use transparent background - HoverOpacity handles the background drawing
    let toolbar = cosmic::widget::container(toolbar_content).class(
        cosmic::theme::Container::Custom(Box::new(|theme| {
            let theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: None, // HoverOpacity draws the background with opacity
                text_color: Some(theme.background.component.on.into()),
                border: Border::default(),
                ..Default::default()
            }
        })),
    );

    HoverOpacity::new(toolbar)
        .force_opaque(force_toolbar_opaque)
        .into()
}

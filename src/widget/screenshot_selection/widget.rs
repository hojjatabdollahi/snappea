//! Refactored ScreenshotSelection widget
//!
//! Uses grouped state structs and a single event handler instead of
//! 96+ individual fields and callbacks.

use std::collections::HashMap;

use cosmic::{
    Element,
    cosmic_theme::Spacing,
    iced::{self, window},
    iced_core::{
        Background, ContentFit, Degrees, Layout, Length, Point, Size, alignment, gradient::Linear,
        layout, overlay, widget::Tree,
    },
    iced_widget::canvas,
    widget::{Row, button, horizontal_space, image, layer_container},
};
use cosmic_bg_config::Source;

use crate::{
    capture::image::ScreenshotImage,
    config::{ShapeTool, ToolbarPosition},
    core::app::OutputState,
    domain::{Choice, DragState, Rect},
    session::{
        messages::Msg,
        state::{AnnotationState, DetectionState, UiState},
    },
};

use super::events::ScreenshotEvent;
use super::helpers::{
    calculate_selection_rect, create_output_rect, filter_ocr_overlays_for_output,
    filter_qr_codes_for_output, get_window_image_info,
};
use crate::render::mesh::{draw_arrow_preview, draw_arrows};
use crate::widget::{
    drawing::{draw_inactive_overlay_with_hint, draw_selection_frame_with_handles},
    output_selection::OutputSelection,
    overlays::{
        ShapesOverlay,
        redact_overlays::{
            PixelationSource, draw_pixelation_preview, draw_redaction_preview,
            draw_redactions_and_pixelations,
        },
        status_overlays::{
            draw_ocr_overlays, draw_ocr_status_indicator, draw_qr_code_overlays,
            draw_qr_scanning_indicator,
        },
    },
    rectangle_selection::RectangleSelection,
    screenshot::SelectedImageWidget,
    settings_drawer::build_settings_drawer,
    tool_button::{build_redact_popup, build_shape_popup},
    toolbar::build_toolbar,
};

/// Output context for multi-monitor support
#[derive(Clone, Debug)]
pub struct OutputContext {
    pub output_count: usize,
    pub highlighted_window_index: usize,
    pub focused_output_index: usize,
    pub current_output_index: usize,
    pub is_active_output: bool,
    pub has_confirmed_selection: bool,
}

/// Check if a string looks like a URL
fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www.")
}

/// The refactored ScreenshotSelection widget
///
/// Instead of 96+ individual fields and callbacks, this uses:
/// - References to grouped state structs
/// - A single event handler that receives ScreenshotEvent
pub struct ScreenshotSelectionWidget<'a, E>
where
    E: Fn(ScreenshotEvent) -> Msg + 'a,
{
    id: cosmic::widget::Id,

    // Core state
    pub choice: Choice,
    pub output: &'a OutputState,
    pub window_id: window::Id,
    pub spacing: Spacing,
    pub dnd_id: u128,

    // Image references
    pub screenshot_image: &'a ScreenshotImage,
    pub toplevel_images: &'a HashMap<String, Vec<ScreenshotImage>>,

    // Grouped state (references to session state)
    pub annotations: &'a AnnotationState,
    pub detection: &'a DetectionState,
    pub ui: &'a UiState,

    // Output context
    pub output_ctx: OutputContext,

    // Computed state
    pub has_any_annotations: bool,
    pub has_any_redactions: bool,
    pub has_ocr_text: bool,

    // Single event handler
    on_event: E,

    // Cached computed values
    output_rect: Rect,
    selection_rect: Option<(f32, f32, f32, f32)>,
    image_scale: f32,
    show_qr_overlays: bool,
    qr_codes_for_output: Vec<(f32, f32, String)>,
    ocr_overlays_for_output: Vec<(f32, f32, f32, f32, i32)>,
    window_image: Option<&'a ::image::RgbaImage>,
    window_display_info: Option<(f32, f32, f32, f32, f32)>,

    // Pre-built child elements
    bg_element: Element<'a, Msg>,
    fg_element: Element<'a, Msg>,
    menu_element: Element<'a, Msg>,
    shapes_element: Element<'a, Msg>,
    settings_drawer_element: Option<Element<'a, Msg>>,
    shape_popup_element: Option<Element<'a, Msg>>,
    redact_popup_element: Option<Element<'a, Msg>>,
}

impl<'a, E> ScreenshotSelectionWidget<'a, E>
where
    E: Fn(ScreenshotEvent) -> Msg + Clone + 'static,
{
    /// Create a new widget
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        choice: Choice,
        screenshot_image: &'a ScreenshotImage,
        toplevel_images: &'a HashMap<String, Vec<ScreenshotImage>>,
        output: &'a OutputState,
        window_id: window::Id,
        spacing: Spacing,
        dnd_id: u128,
        annotations: &'a AnnotationState,
        detection: &'a DetectionState,
        ui: &'a UiState,
        output_ctx: OutputContext,
        has_any_annotations: bool,
        has_any_redactions: bool,
        has_ocr_text: bool,
        on_event: E,
    ) -> Self {
        let output_rect = create_output_rect(output.logical_pos, output.logical_size);
        let image_scale = screenshot_image.rgba.width() as f32 / output.logical_size.0 as f32;

        // Build QR overlay - only show when not actively dragging a rectangle
        let show_qr_overlays = match choice {
            Choice::Rectangle(_, DragState::None) => true,
            Choice::Rectangle(_, _) => false,
            _ => true,
        };

        // Filter QR codes and OCR overlays for this output
        let qr_codes_for_output = filter_qr_codes_for_output(&detection.qr_codes, &output.name);
        let ocr_overlays_for_output =
            filter_ocr_overlays_for_output(&detection.ocr_overlays, &output.name);

        // Calculate selection rectangle relative to this output
        let selection_rect =
            calculate_selection_rect(&choice, output_rect, output.logical_size, toplevel_images);

        // Get window image and display info for correct pixelation preview
        let (window_image, window_display_info) =
            get_window_image_info(&choice, output.logical_size, toplevel_images);

        let space_l = spacing.space_l;
        let space_s = spacing.space_s;
        let space_xs = spacing.space_xs;
        let space_xxs = spacing.space_xxs;

        // Build bg_element
        let bg_element = match &choice {
            Choice::Output(_) | Choice::Rectangle(..) | Choice::Window(_, Some(_)) => {
                image::Image::new(screenshot_image.handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            }
            Choice::Window(_, None) => match output.bg_source.clone() {
                Some(Source::Path(path)) => image::Image::new(image::Handle::from_path(path))
                    .content_fit(ContentFit::Cover)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
                Some(Source::Color(color)) => {
                    layer_container(horizontal_space().width(Length::Fill))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .class(cosmic::theme::Container::Custom(Box::new(move |_| {
                            let color = color.clone();
                            cosmic::iced_widget::container::Style {
                                background: Some(match color {
                                    cosmic_bg_config::Color::Single(c) => Background::Color(
                                        cosmic::iced::Color::new(c[0], c[1], c[2], 1.0),
                                    ),
                                    cosmic_bg_config::Color::Gradient(
                                        cosmic_bg_config::Gradient { colors, radius },
                                    ) => {
                                        let stop_increment = 1.0 / (colors.len() - 1) as f32;
                                        let mut stop = 0.0;
                                        let mut linear = Linear::new(Degrees(radius));
                                        for &[r, g, b] in colors.iter() {
                                            linear = linear.add_stop(
                                                stop,
                                                cosmic::iced::Color::from_rgb(r, g, b),
                                            );
                                            stop += stop_increment;
                                        }
                                        Background::Gradient(cosmic::iced_core::Gradient::Linear(
                                            linear,
                                        ))
                                    }
                                }),
                                ..Default::default()
                            }
                        })))
                        .into()
                }
                None => image::Image::new(image::Handle::from_path(
                    "/usr/share/backgrounds/pop/kate-hazen-COSMIC-desktop-wallpaper.png",
                ))
                .content_fit(ContentFit::Cover)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            },
        };

        // Build fg_element
        let on_event_clone = on_event.clone();
        let fg_element: Element<'a, Msg> = match choice.clone() {
            Choice::Rectangle(r, drag_state) => RectangleSelection::new(
                output_rect,
                r,
                drag_state,
                window_id,
                dnd_id,
                move |s, r| {
                    on_event_clone(ScreenshotEvent::choice_changed(Choice::Rectangle(r, s)))
                },
                &screenshot_image.rgba,
                image_scale,
                annotations.arrow_mode,
                annotations.redact_mode,
                annotations.pixelate_mode,
                annotations.circle_mode,
                annotations.rect_outline_mode,
                ui.shape_popup_open || ui.redact_popup_open || ui.settings_drawer_open,
                ui.magnifier_enabled,
            )
            .into(),
            Choice::Output(None) => {
                let is_focused = output_ctx.current_output_index == output_ctx.focused_output_index;
                OutputSelection::new(on_event(ScreenshotEvent::output_changed(
                    output.output.clone(),
                )))
                .picker_mode(true)
                .focused(is_focused)
                .on_click(on_event(ScreenshotEvent::confirm()))
                .into()
            }
            Choice::Output(Some(ref selected_output)) => {
                let is_selected = selected_output == &output.name;
                OutputSelection::new(on_event(ScreenshotEvent::output_changed(
                    output.output.clone(),
                )))
                .picker_mode(false)
                .selected(is_selected)
                .into()
            }
            Choice::Window(_, None) => {
                let imgs = toplevel_images
                    .get(&output.name)
                    .map(|x| x.as_slice())
                    .unwrap_or_default();
                let total_img_width = imgs.iter().map(|img| img.width()).sum::<u32>().max(1);
                let is_focused_output =
                    output_ctx.current_output_index == output_ctx.focused_output_index;

                let img_buttons = imgs.iter().enumerate().map(|(i, img)| {
                    let portion =
                        (img.width() as u64 * u16::MAX as u64 / total_img_width as u64).max(1);
                    let is_highlighted =
                        is_focused_output && i == output_ctx.highlighted_window_index;
                    let output_name = output.name.clone();
                    layer_container(
                        button::custom(
                            image::Image::new(img.handle.clone())
                                .content_fit(ContentFit::ScaleDown),
                        )
                        .on_press(on_event(ScreenshotEvent::window_selected(output_name, i)))
                        .selected(is_highlighted)
                        .class(cosmic::theme::Button::Image),
                    )
                    .align_x(alignment::Alignment::Center)
                    .width(Length::FillPortion(portion as u16))
                    .height(Length::Shrink)
                    .into()
                });
                layer_container(
                    Row::with_children(img_buttons)
                        .spacing(space_l)
                        .width(Length::Fill)
                        .align_y(alignment::Alignment::Center)
                        .padding(space_l),
                )
                .align_x(alignment::Alignment::Center)
                .align_y(alignment::Alignment::Center)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            Choice::Window(ref win_output, Some(win_index)) if win_output == &output.name => {
                let screen_size = (output.logical_size.0, output.logical_size.1);
                SelectedImageWidget::new(
                    win_output.clone(),
                    Some(win_index),
                    toplevel_images,
                    screen_size,
                )
                .into()
            }
            Choice::Window(_, Some(_)) => cosmic::widget::horizontal_space()
                .width(Length::Fill)
                .into(),
        };

        // Build menu_element
        let has_selection = match &choice {
            Choice::Rectangle(r, _) => r.dimensions().is_some(),
            Choice::Window(_, Some(_)) => true,
            Choice::Output(Some(_)) => true,
            _ => false,
        };

        let shape_mode_active = match ui.primary_shape_tool {
            ShapeTool::Arrow => annotations.arrow_mode,
            ShapeTool::Circle => annotations.circle_mode,
            ShapeTool::Rectangle => annotations.rect_outline_mode,
        };

        let redact_mode_active = match ui.primary_redact_tool {
            crate::config::RedactTool::Redact => annotations.redact_mode,
            crate::config::RedactTool::Pixelate => annotations.pixelate_mode,
        };

        let on_event_clone2 = on_event.clone();
        let menu_element = build_toolbar(
            choice.clone(),
            output.name.clone(),
            ui.toolbar_position,
            has_selection,
            has_ocr_text,
            &detection.qr_codes,
            ui.primary_shape_tool,
            shape_mode_active,
            ui.shape_popup_open,
            ui.primary_redact_tool,
            redact_mode_active,
            ui.redact_popup_open,
            space_s,
            space_xs,
            space_xxs,
            move |c| on_event_clone2(ScreenshotEvent::choice_changed(c)),
            on_event(ScreenshotEvent::copy_to_clipboard()),
            on_event(ScreenshotEvent::save_to_pictures()),
            on_event(ScreenshotEvent::record_region()),
            on_event(ScreenshotEvent::shape_mode_toggle()),
            on_event(ScreenshotEvent::shape_popup_toggle()),
            on_event(ScreenshotEvent::redact_tool_mode_toggle()),
            on_event(ScreenshotEvent::redact_popup_toggle()),
            on_event(ScreenshotEvent::ocr_requested()),
            on_event(ScreenshotEvent::ocr_copy_and_close()),
            on_event(ScreenshotEvent::qr_requested()),
            on_event(ScreenshotEvent::qr_copy_and_close()),
            on_event(ScreenshotEvent::cancel()),
            &{
                let on_event = on_event.clone();
                move |pos| on_event(ScreenshotEvent::toolbar_position(pos))
            },
            on_event(ScreenshotEvent::settings_drawer_toggle()),
            ui.settings_drawer_open,
            ui.settings_drawer_open || ui.shape_popup_open || ui.redact_popup_open,
            output_ctx.output_count,
            ui.tesseract_available,
            ui.is_video_mode,
            {
                let on_event = on_event.clone();
                move |is_video| on_event(ScreenshotEvent::capture_mode_toggle(is_video))
            },
        );

        // Build shapes_element
        let on_event_c1 = on_event.clone();
        let on_event_c2 = on_event.clone();
        let on_event_r1 = on_event.clone();
        let on_event_r2 = on_event.clone();
        let shapes_element = {
            let program = ShapesOverlay {
                selection_rect,
                output_rect,
                circles: annotations.circles.clone(),
                rect_outlines: annotations.rect_outlines.clone(),
                circle_mode: annotations.circle_mode,
                rect_outline_mode: annotations.rect_outline_mode,
                circle_drawing: annotations.circle_drawing,
                rect_outline_drawing: annotations.rect_outline_drawing,
                on_circle_start: Some(Box::new(move |x, y| {
                    on_event_c1(ScreenshotEvent::circle_start(x, y))
                })),
                on_circle_end: Some(Box::new(move |x, y| {
                    on_event_c2(ScreenshotEvent::circle_end(x, y))
                })),
                on_rect_start: Some(Box::new(move |x, y| {
                    on_event_r1(ScreenshotEvent::rectangle_start(x, y))
                })),
                on_rect_end: Some(Box::new(move |x, y| {
                    on_event_r2(ScreenshotEvent::rectangle_end(x, y))
                })),
                shape_color: ui.shape_color,
                shape_shadow: ui.shape_shadow,
            };

            canvas::Canvas::new(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        // Build settings_drawer_element
        const REPOSITORY: &str = "https://github.com/hojjatabdollahi/snappea";
        let settings_drawer_element = if ui.settings_drawer_open {
            let on_event_encoder = on_event.clone();
            let on_event_container = on_event.clone();
            let on_event_framerate = on_event.clone();

            Some(build_settings_drawer(
                ui.toolbar_position,
                ui.magnifier_enabled,
                on_event(ScreenshotEvent::magnifier_toggle()),
                ui.save_location_setting,
                on_event(ScreenshotEvent::save_location_pictures()),
                on_event(ScreenshotEvent::save_location_documents()),
                ui.copy_to_clipboard_on_save,
                on_event(ScreenshotEvent::copy_on_save_toggle()),
                on_event(ScreenshotEvent::open_url(REPOSITORY.to_string())),
                // Recording settings
                ui.encoder_displays.clone(),
                ui.selected_encoder.clone(),
                move |e| on_event_encoder(ScreenshotEvent::video_encoder_set(e)),
                ui.video_container,
                move |c| on_event_container(ScreenshotEvent::video_container_set(c)),
                ui.video_framerate,
                move |f| on_event_framerate(ScreenshotEvent::video_framerate_set(f)),
                ui.video_show_cursor,
                on_event(ScreenshotEvent::show_cursor_toggle()),
                space_s,
                space_xs,
            ))
        } else {
            None
        };

        // Build shape_popup_element
        let on_event_color = on_event.clone();
        let shape_popup_element = if ui.shape_popup_open {
            Some(build_shape_popup(
                ui.primary_shape_tool,
                ui.shape_color,
                ui.shape_shadow,
                has_any_annotations,
                on_event(ScreenshotEvent::shape_tool_set(ShapeTool::Arrow)),
                on_event(ScreenshotEvent::shape_tool_set(ShapeTool::Circle)),
                on_event(ScreenshotEvent::shape_tool_set(ShapeTool::Rectangle)),
                &move |c| on_event_color(ScreenshotEvent::shape_color_set(c)),
                on_event(ScreenshotEvent::shape_shadow_toggle()),
                on_event(ScreenshotEvent::clear_shapes()),
                space_s,
                space_xs,
            ))
        } else {
            None
        };

        // Build redact_popup_element
        let on_event_size = on_event.clone();
        let redact_popup_element = if ui.redact_popup_open {
            Some(build_redact_popup(
                ui.primary_redact_tool,
                has_any_redactions,
                ui.pixelation_block_size,
                on_event(ScreenshotEvent::redact_tool_set(
                    crate::config::RedactTool::Redact,
                )),
                on_event(ScreenshotEvent::redact_tool_set(
                    crate::config::RedactTool::Pixelate,
                )),
                move |size| on_event_size(ScreenshotEvent::pixelation_size_set(size)),
                on_event(ScreenshotEvent::pixelation_size_save()),
                on_event(ScreenshotEvent::clear_redactions()),
                space_s,
                space_xs,
            ))
        } else {
            None
        };

        Self {
            id: cosmic::widget::Id::unique(),
            choice,
            output,
            window_id,
            spacing,
            dnd_id,
            screenshot_image,
            toplevel_images,
            annotations,
            detection,
            ui,
            output_ctx,
            has_any_annotations,
            has_any_redactions,
            has_ocr_text,
            on_event,
            output_rect,
            selection_rect,
            image_scale,
            show_qr_overlays,
            qr_codes_for_output,
            ocr_overlays_for_output,
            window_image,
            window_display_info,
            bg_element,
            fg_element,
            menu_element,
            shapes_element,
            settings_drawer_element,
            shape_popup_element,
            redact_popup_element,
        }
    }

    /// Emit an event through the handler
    fn emit(&self, event: ScreenshotEvent) -> Msg {
        (self.on_event)(event)
    }

    // Helper methods to check current mode
    fn is_arrow_mode(&self) -> bool {
        self.annotations.arrow_mode
    }

    fn is_circle_mode(&self) -> bool {
        self.annotations.circle_mode
    }

    fn is_rectangle_mode(&self) -> bool {
        self.annotations.rect_outline_mode
    }

    fn is_redact_mode(&self) -> bool {
        self.annotations.redact_mode
    }

    fn is_pixelate_mode(&self) -> bool {
        self.annotations.pixelate_mode
    }

    fn is_any_drawing_mode(&self) -> bool {
        self.is_arrow_mode()
            || self.is_circle_mode()
            || self.is_rectangle_mode()
            || self.is_redact_mode()
            || self.is_pixelate_mode()
    }
}

// ============================================================================
// Widget trait implementation
// ============================================================================

impl<'a, E> cosmic::iced_core::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for ScreenshotSelectionWidget<'a, E>
where
    E: Fn(ScreenshotEvent) -> Msg + Clone + 'static,
{
    fn children(&self) -> Vec<Tree> {
        let mut children = vec![
            Tree::new(&self.bg_element),
            Tree::new(&self.fg_element),
            Tree::new(&self.shapes_element),
            Tree::new(&self.menu_element),
        ];
        if let Some(ref drawer) = self.settings_drawer_element {
            children.push(Tree::new(drawer));
        }
        if let Some(ref selector) = self.shape_popup_element {
            children.push(Tree::new(selector));
        }
        if let Some(ref popup) = self.redact_popup_element {
            children.push(Tree::new(popup));
        }
        children
    }

    fn diff(&mut self, tree: &mut Tree) {
        let mut elements: Vec<&mut Element<'_, Msg>> = vec![
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.shapes_element,
            &mut self.menu_element,
        ];
        if let Some(ref mut drawer) = self.settings_drawer_element {
            elements.push(drawer);
        }
        if let Some(ref mut selector) = self.shape_popup_element {
            elements.push(selector);
        }
        if let Some(ref mut popup) = self.redact_popup_element {
            elements.push(popup);
        }
        tree.diff_children(&mut elements);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let children = &mut tree.children;
        let bg_node = self
            .bg_element
            .as_widget()
            .layout(&mut children[0], renderer, limits);
        let fg_node = self
            .fg_element
            .as_widget()
            .layout(&mut children[1], renderer, limits);
        let shapes_node =
            self.shapes_element
                .as_widget()
                .layout(&mut children[2], renderer, limits);
        let mut menu_node =
            self.menu_element
                .as_widget()
                .layout(&mut children[3], renderer, limits);

        let menu_bounds = menu_node.bounds();
        let margin = 32.0_f32;

        // Position menu based on toolbar_position
        let menu_pos = match self.ui.toolbar_position {
            ToolbarPosition::Bottom => Point {
                x: (limits.max().width - menu_bounds.width) / 2.0,
                y: limits.max().height - menu_bounds.height - margin,
            },
            ToolbarPosition::Top => Point {
                x: (limits.max().width - menu_bounds.width) / 2.0,
                y: margin,
            },
            ToolbarPosition::Left => Point {
                x: margin,
                y: (limits.max().height - menu_bounds.height) / 2.0,
            },
            ToolbarPosition::Right => Point {
                x: limits.max().width - menu_bounds.width - margin,
                y: (limits.max().height - menu_bounds.height) / 2.0,
            },
        };
        menu_node = menu_node.move_to(menu_pos);

        let mut nodes = vec![bg_node, fg_node, shapes_node, menu_node.clone()];

        // Layout settings drawer if present
        if let Some(ref drawer) = self.settings_drawer_element {
            let mut drawer_node = drawer
                .as_widget()
                .layout(&mut children[4], renderer, limits);
            let drawer_bounds = drawer_node.bounds();
            let drawer_margin = 8.0_f32;
            let button_offset = 40.0 + 8.0;

            let drawer_pos = match self.ui.toolbar_position {
                ToolbarPosition::Bottom => {
                    let settings_btn_x = menu_pos.x + menu_bounds.width - button_offset - 20.0;
                    Point {
                        x: (settings_btn_x - drawer_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - drawer_bounds.width - margin),
                        y: menu_pos.y - drawer_bounds.height - drawer_margin,
                    }
                }
                ToolbarPosition::Top => {
                    let settings_btn_x = menu_pos.x + menu_bounds.width - button_offset - 20.0;
                    Point {
                        x: (settings_btn_x - drawer_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - drawer_bounds.width - margin),
                        y: menu_pos.y + menu_bounds.height + drawer_margin,
                    }
                }
                ToolbarPosition::Left => {
                    let settings_btn_y = menu_pos.y + menu_bounds.height - button_offset - 20.0;
                    Point {
                        x: menu_pos.x + menu_bounds.width + drawer_margin,
                        y: (settings_btn_y - drawer_bounds.height / 2.0)
                            .max(margin)
                            .min(limits.max().height - drawer_bounds.height - margin),
                    }
                }
                ToolbarPosition::Right => {
                    let settings_btn_y = menu_pos.y + menu_bounds.height - button_offset - 20.0;
                    Point {
                        x: menu_pos.x - drawer_bounds.width - drawer_margin,
                        y: (settings_btn_y - drawer_bounds.height / 2.0)
                            .max(margin)
                            .min(limits.max().height - drawer_bounds.height - margin),
                    }
                }
            };
            drawer_node = drawer_node.move_to(drawer_pos);
            nodes.push(drawer_node);
        }

        // Layout shape selector popup if present
        if let Some(ref selector) = self.shape_popup_element {
            let child_idx = if self.settings_drawer_element.is_some() {
                5
            } else {
                4
            };
            let mut selector_node =
                selector
                    .as_widget()
                    .layout(&mut children[child_idx], renderer, limits);
            let selector_bounds = selector_node.bounds();
            let selector_margin = 4.0_f32;
            let shapes_btn_fraction = 0.42_f32;

            let selector_pos = match self.ui.toolbar_position {
                ToolbarPosition::Bottom => {
                    let shapes_btn_x = menu_pos.x + menu_bounds.width * shapes_btn_fraction;
                    Point {
                        x: (shapes_btn_x - selector_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - selector_bounds.width - margin),
                        y: menu_pos.y - selector_bounds.height - selector_margin,
                    }
                }
                ToolbarPosition::Top => {
                    let shapes_btn_x = menu_pos.x + menu_bounds.width * shapes_btn_fraction;
                    Point {
                        x: (shapes_btn_x - selector_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - selector_bounds.width - margin),
                        y: menu_pos.y + menu_bounds.height + selector_margin,
                    }
                }
                ToolbarPosition::Left => {
                    let shapes_btn_y = menu_pos.y + menu_bounds.height * shapes_btn_fraction;
                    Point {
                        x: menu_pos.x + menu_bounds.width + selector_margin,
                        y: (shapes_btn_y - selector_bounds.height / 2.0)
                            .max(margin)
                            .min(limits.max().height - selector_bounds.height - margin),
                    }
                }
                ToolbarPosition::Right => {
                    let shapes_btn_y = menu_pos.y + menu_bounds.height * shapes_btn_fraction;
                    Point {
                        x: menu_pos.x - selector_bounds.width - selector_margin,
                        y: (shapes_btn_y - selector_bounds.height / 2.0)
                            .max(margin)
                            .min(limits.max().height - selector_bounds.height - margin),
                    }
                }
            };
            selector_node = selector_node.move_to(selector_pos);
            nodes.push(selector_node);
        }

        // Layout redact popup if present
        if let Some(ref popup) = self.redact_popup_element {
            let mut child_idx = 4;
            if self.settings_drawer_element.is_some() {
                child_idx += 1;
            }
            if self.shape_popup_element.is_some() {
                child_idx += 1;
            }
            let mut popup_node =
                popup
                    .as_widget()
                    .layout(&mut children[child_idx], renderer, limits);
            let popup_bounds = popup_node.bounds();
            let popup_margin = 4.0_f32;
            let redact_btn_fraction = 0.52_f32;

            let popup_pos = match self.ui.toolbar_position {
                ToolbarPosition::Bottom => {
                    let redact_btn_x = menu_pos.x + menu_bounds.width * redact_btn_fraction;
                    Point {
                        x: (redact_btn_x - popup_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - popup_bounds.width - margin),
                        y: menu_pos.y - popup_bounds.height - popup_margin,
                    }
                }
                ToolbarPosition::Top => {
                    let redact_btn_x = menu_pos.x + menu_bounds.width * redact_btn_fraction;
                    Point {
                        x: (redact_btn_x - popup_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - popup_bounds.width - margin),
                        y: menu_pos.y + menu_bounds.height + popup_margin,
                    }
                }
                ToolbarPosition::Left => {
                    let redact_btn_y = menu_pos.y + menu_bounds.height * redact_btn_fraction;
                    Point {
                        x: menu_pos.x + menu_bounds.width + popup_margin,
                        y: (redact_btn_y - popup_bounds.height / 2.0)
                            .max(margin)
                            .min(limits.max().height - popup_bounds.height - margin),
                    }
                }
                ToolbarPosition::Right => {
                    let redact_btn_y = menu_pos.y + menu_bounds.height * redact_btn_fraction;
                    Point {
                        x: menu_pos.x - popup_bounds.width - popup_margin,
                        y: (redact_btn_y - popup_bounds.height / 2.0)
                            .max(margin)
                            .min(limits.max().height - popup_bounds.height - margin),
                    }
                }
            };
            popup_node = popup_node.move_to(popup_pos);
            nodes.push(popup_node);
        }

        layout::Node::with_children(
            limits.resolve(Length::Fill, Length::Fill, Size::ZERO),
            nodes,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        cursor: cosmic::iced::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
    ) {
        use cosmic::iced_core::Renderer;

        let children = &[
            &self.bg_element,
            &self.fg_element,
            &self.shapes_element,
            &self.menu_element,
        ];
        let mut children_iter = layout.children().zip(children).enumerate();

        // Draw bg_element first
        let bg_info = children_iter.next();
        if let Some((i, (layout, child))) = bg_info {
            let bg_tree = &tree.children[i];
            child
                .as_widget()
                .draw(bg_tree, renderer, theme, style, layout, cursor, viewport);
        }

        // If this is not the active output and there's a confirmed selection, draw a dark overlay
        if !self.output_ctx.is_active_output && self.output_ctx.has_confirmed_selection {
            let bounds = layout.bounds();
            renderer.with_layer(bounds, |renderer| {
                draw_inactive_overlay_with_hint(
                    renderer,
                    bounds,
                    "Press 'S' or Screen button to change selection",
                    0.7,
                );
            });
            return;
        }

        // Get fg_element info
        let fg_info = children_iter.next();

        // In window mode, draw fg_element (SelectedImageWidget) FIRST so annotations appear on top
        // In other modes, we draw fg_element later (after annotations) since it's just selection handles
        let is_window_mode = matches!(self.choice, Choice::Window(_, Some(_)));
        if is_window_mode {
            if let Some((i, (layout, child))) = &fg_info {
                renderer.with_layer(layout.bounds(), |renderer| {
                    let tree = &tree.children[*i];
                    child
                        .as_widget()
                        .draw(tree, renderer, theme, style, *layout, cursor, viewport);
                });
            }
        }

        // Draw redactions and pixelations
        let output_offset = (self.output_rect.left as f32, self.output_rect.top as f32);
        let pixelation_source =
            if let (Some(win_img), Some((win_x, win_y, _win_w, _win_h, display_to_img_scale))) =
                (self.window_image, self.window_display_info)
            {
                PixelationSource::Window {
                    image: win_img,
                    offset: (win_x, win_y),
                    scale: display_to_img_scale,
                }
            } else {
                PixelationSource::Screenshot {
                    image: &self.screenshot_image.rgba,
                    scale: self.image_scale,
                }
            };

        draw_redactions_and_pixelations(
            renderer,
            viewport,
            &self.annotations.annotations[..self.annotations.annotation_index],
            output_offset,
            &pixelation_source,
        );

        // Draw pixelation preview
        if let Some((start_x, start_y)) = self.annotations.pixelate_drawing
            && let Some(cursor_pos) = cursor.position()
        {
            draw_pixelation_preview(
                renderer,
                viewport,
                (start_x, start_y),
                (cursor_pos.x, cursor_pos.y),
                output_offset,
                self.ui.pixelation_block_size,
                &pixelation_source,
            );
        }

        // Draw redaction preview
        if let Some((start_x, start_y)) = self.annotations.redact_drawing
            && let Some(cursor_pos) = cursor.position()
        {
            draw_redaction_preview(
                renderer,
                viewport,
                (start_x, start_y),
                (cursor_pos.x, cursor_pos.y),
                output_offset,
            );
        }

        // Draw shapes canvas overlay
        if let Some((i, (layout, child))) = children_iter.next() {
            renderer.with_layer(layout.bounds(), |renderer| {
                let tree = &tree.children[i];
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        }

        // Draw arrows
        draw_arrows(renderer, viewport, &self.annotations.arrows, output_offset);

        // Draw arrow preview
        if let Some((start_x, start_y)) = self.annotations.arrow_drawing
            && let Some(cursor_pos) = cursor.position()
        {
            let local_start = (
                start_x - self.output_rect.left as f32,
                start_y - self.output_rect.top as f32,
            );
            let local_end = (cursor_pos.x, cursor_pos.y);
            let shape_color: cosmic::iced::Color = self.ui.shape_color.into();
            draw_arrow_preview(
                renderer,
                viewport,
                local_start,
                local_end,
                shape_color,
                self.ui.shape_shadow,
            );
        }

        // Draw fg_element for non-window modes (selection UI above annotations)
        // In window mode, fg_element was already drawn earlier so annotations appear on top
        if !is_window_mode {
            if let Some((i, (layout, child))) = fg_info {
                renderer.with_layer(layout.bounds(), |renderer| {
                    let tree = &tree.children[i];
                    child
                        .as_widget()
                        .draw(tree, renderer, theme, style, layout, cursor, viewport);
                });
            }
        }

        let cosmic_theme = theme.cosmic();
        let accent_color: cosmic::iced::Color = cosmic_theme.accent_color().into();
        let corner_radius: f32 = cosmic_theme.corner_radii.radius_s[0];

        // Draw QR scanning status or QR overlays
        if self.show_qr_overlays {
            if self.detection.qr_scanning {
                draw_qr_scanning_indicator(renderer, viewport, accent_color, corner_radius);
            }

            if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                draw_qr_code_overlays(
                    renderer,
                    viewport,
                    &self.qr_codes_for_output,
                    (sel_x, sel_y, sel_w, sel_h),
                    accent_color,
                    corner_radius,
                );
            }
        }

        // Show OCR status indicator
        draw_ocr_status_indicator(
            renderer,
            viewport,
            &self.detection.ocr_status,
            self.detection.qr_scanning,
            accent_color,
            corner_radius,
        );

        // Draw OCR overlays
        if self.show_qr_overlays {
            draw_ocr_overlays(renderer, viewport, &self.ocr_overlays_for_output);
        }

        // Draw selection frame
        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
            let output_width = (self.output_rect.right - self.output_rect.left) as f32;
            let output_height = (self.output_rect.bottom - self.output_rect.top) as f32;
            draw_selection_frame_with_handles(
                renderer,
                (sel_x, sel_y, sel_w, sel_h),
                (output_width, output_height),
                accent_color,
                corner_radius,
            );
        }

        // Draw menu
        let _ = children_iter;
        if let Some((i, (layout, child))) = children_iter.next() {
            renderer.with_layer(layout.bounds(), |renderer| {
                let tree = &tree.children[i];
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        }

        // Draw settings drawer
        if let Some(ref drawer) = self.settings_drawer_element {
            let layout_children: Vec<_> = layout.children().collect();
            if layout_children.len() > 4 {
                let drawer_layout = layout_children[4];
                renderer.with_layer(drawer_layout.bounds(), |renderer| {
                    let drawer_tree = &tree.children[4];
                    drawer.as_widget().draw(
                        drawer_tree,
                        renderer,
                        theme,
                        style,
                        drawer_layout,
                        cursor,
                        viewport,
                    );
                });
            }
        }

        // Draw shape selector popup
        if let Some(ref selector) = self.shape_popup_element {
            let layout_children: Vec<_> = layout.children().collect();
            let selector_idx = if self.settings_drawer_element.is_some() {
                5
            } else {
                4
            };
            if layout_children.len() > selector_idx {
                let selector_layout = layout_children[selector_idx];
                renderer.with_layer(selector_layout.bounds(), |renderer| {
                    let selector_tree = &tree.children[selector_idx];
                    selector.as_widget().draw(
                        selector_tree,
                        renderer,
                        theme,
                        style,
                        selector_layout,
                        cursor,
                        viewport,
                    );
                });
            }
        }

        // Draw redact popup
        if let Some(ref popup) = self.redact_popup_element {
            let layout_children: Vec<_> = layout.children().collect();
            let mut popup_idx = 4;
            if self.settings_drawer_element.is_some() {
                popup_idx += 1;
            }
            if self.shape_popup_element.is_some() {
                popup_idx += 1;
            }
            if layout_children.len() > popup_idx {
                let popup_layout = layout_children[popup_idx];
                renderer.with_layer(popup_layout.bounds(), |renderer| {
                    let popup_tree = &tree.children[popup_idx];
                    popup.as_widget().draw(
                        popup_tree,
                        renderer,
                        theme,
                        style,
                        popup_layout,
                        cursor,
                        viewport,
                    );
                });
            }
        }
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: cosmic::iced_core::Event,
        layout: Layout<'_>,
        cursor: cosmic::iced::mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn cosmic::iced_core::Clipboard,
        shell: &mut cosmic::iced_core::Shell<'_, Msg>,
        viewport: &cosmic::iced_core::Rectangle,
    ) -> cosmic::iced_core::event::Status {
        use cosmic::iced_core::Event;
        use cosmic::iced_core::mouse::{Button, Event as MouseEvent};

        // Handle click-outside-to-close for popups (both left and right click)
        if let Event::Mouse(MouseEvent::ButtonPressed(button)) = &event
            && matches!(button, Button::Left | Button::Right)
            && let Some(pos) = cursor.position()
        {
            let layout_children: Vec<_> = layout.children().collect();

            // Handle shape selector popup click-outside
            if self.ui.shape_popup_open {
                let selector_idx = if self.settings_drawer_element.is_some() {
                    5
                } else {
                    4
                };
                let inside_selector = if layout_children.len() > selector_idx {
                    layout_children[selector_idx].bounds().contains(pos)
                } else {
                    false
                };
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                if !inside_selector && !inside_toolbar {
                    shell.publish(self.emit(ScreenshotEvent::shape_popup_close()));
                    return cosmic::iced_core::event::Status::Captured;
                }
            }

            // Handle redact popup click-outside
            if self.ui.redact_popup_open {
                let mut popup_idx = 4;
                if self.settings_drawer_element.is_some() {
                    popup_idx += 1;
                }
                if self.shape_popup_element.is_some() {
                    popup_idx += 1;
                }
                let inside_popup = if layout_children.len() > popup_idx {
                    layout_children[popup_idx].bounds().contains(pos)
                } else {
                    false
                };
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                if !inside_popup && !inside_toolbar {
                    shell.publish(self.emit(ScreenshotEvent::redact_popup_close()));
                    return cosmic::iced_core::event::Status::Captured;
                }
            }

            // Handle settings drawer click-outside
            if self.ui.settings_drawer_open {
                let inside_drawer = if layout_children.len() > 4 {
                    layout_children[4].bounds().contains(pos)
                } else {
                    false
                };
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                if !inside_drawer && !inside_toolbar {
                    shell.publish(self.emit(ScreenshotEvent::settings_drawer_toggle()));
                    return cosmic::iced_core::event::Status::Captured;
                }
            }
        }

        // Block mouse events on non-active outputs
        if !self.output_ctx.is_active_output && self.output_ctx.has_confirmed_selection {
            if matches!(&event, Event::Mouse(_)) {
                return cosmic::iced_core::event::Status::Captured;
            }
        }

        // Handle clicks on QR code URL open buttons
        if let Event::Mouse(mouse_event) = &event
            && let Some(pos) = cursor.position()
            && matches!(mouse_event, MouseEvent::ButtonPressed(Button::Left))
            && let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect
        {
            let button_size = 28.0_f32;
            let padding = 8.0;

            for (x, y, content) in &self.qr_codes_for_output {
                if !is_url(content) {
                    continue;
                }

                let font_size = 14.0_f32;
                let button_space = button_size + padding;
                let max_label_width = (sel_w - padding * 4.0 - button_space).clamp(80.0, 400.0);
                let chars_per_line = (max_label_width / (font_size * 0.55)).max(10.0) as usize;
                let num_lines = ((content.len() / chars_per_line).max(1) + 1).min(6);
                let text_height = (num_lines as f32 * font_size * 1.3).min(sel_h * 0.6);
                let bg_width = max_label_width + padding * 2.0 + button_space;
                let bg_height = text_height.max(button_size) + padding * 2.0;

                let mut label_x = *x - bg_width / 2.0;
                let mut label_y = *y - bg_height / 2.0;
                label_x = label_x
                    .max(sel_x + padding)
                    .min(sel_x + sel_w - bg_width - padding);
                label_y = label_y
                    .max(sel_y + padding)
                    .min(sel_y + sel_h - bg_height - padding);

                let button_x = label_x + bg_width - padding - button_size;
                let button_y = label_y + (bg_height - button_size) / 2.0;

                if pos.x >= button_x
                    && pos.x <= button_x + button_size
                    && pos.y >= button_y
                    && pos.y <= button_y + button_size
                {
                    shell.publish(self.emit(ScreenshotEvent::open_url(content.clone())));
                    return cosmic::iced_core::event::Status::Captured;
                }
            }
        }

        // Get layout children for bounds checking
        let layout_children = layout.children().collect::<Vec<_>>();

        // Check if click is inside toolbar bounds - if so, only let the toolbar handle it
        // This prevents clicks on the toolbar from starting rectangle selections behind it
        let click_inside_toolbar = if let Event::Mouse(MouseEvent::ButtonPressed(_)) = &event {
            if let Some(pos) = cursor.position() {
                layout_children.len() > 3 && layout_children[3].bounds().contains(pos)
            } else {
                false
            }
        } else {
            false
        };

        // Let child widgets handle the event
        let mut children: Vec<&mut Element<'_, Msg>> = vec![
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.shapes_element,
            &mut self.menu_element,
        ];
        if let Some(ref mut drawer) = self.settings_drawer_element {
            children.push(drawer);
        }
        if let Some(ref mut selector) = self.shape_popup_element {
            children.push(selector);
        }
        if let Some(ref mut popup) = self.redact_popup_element {
            children.push(popup);
        }

        let mut status = cosmic::iced_core::event::Status::Ignored;
        for (i, (child_layout, child)) in layout_children
            .into_iter()
            .zip(children.into_iter())
            .enumerate()
            .rev()
        {
            // Skip bg_element (0), fg_element (1), and shapes_element (2) if click is inside toolbar
            // Only let menu_element (3) and popups/drawers handle it
            if click_inside_toolbar && i < 3 {
                continue;
            }

            let child_tree = &mut tree.children[i];
            status = child.as_widget_mut().on_event(
                child_tree,
                event.clone(),
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
            if matches!(event, cosmic::iced_core::event::Event::PlatformSpecific(_)) {
                continue;
            }
            if matches!(status, cosmic::iced_core::event::Status::Captured) {
                return status;
            }
        }

        // Capture unhandled clicks inside toolbar and popups to prevent drawing behind them (both left and right)
        if let Event::Mouse(MouseEvent::ButtonPressed(button)) = &event
            && matches!(button, Button::Left | Button::Right)
        {
            // If click was inside toolbar, capture it even if no button handled it
            if click_inside_toolbar {
                return cosmic::iced_core::event::Status::Captured;
            }

            if let Some(pos) = cursor.position() {
                let layout_children: Vec<_> = layout.children().collect();

                // Check shape popup
                if self.ui.shape_popup_open {
                    let selector_idx = if self.settings_drawer_element.is_some() {
                        5
                    } else {
                        4
                    };
                    if layout_children.len() > selector_idx
                        && layout_children[selector_idx].bounds().contains(pos)
                    {
                        return cosmic::iced_core::event::Status::Captured;
                    }
                }

                // Check redact popup
                if self.ui.redact_popup_open {
                    let mut popup_idx = 4;
                    if self.settings_drawer_element.is_some() {
                        popup_idx += 1;
                    }
                    if self.shape_popup_element.is_some() {
                        popup_idx += 1;
                    }
                    if layout_children.len() > popup_idx
                        && layout_children[popup_idx].bounds().contains(pos)
                    {
                        return cosmic::iced_core::event::Status::Captured;
                    }
                }

                // Check settings drawer
                if self.ui.settings_drawer_open {
                    if layout_children.len() > 4 && layout_children[4].bounds().contains(pos) {
                        return cosmic::iced_core::event::Status::Captured;
                    }
                }
            }
        }

        // Handle annotation drawing events
        if let Event::Mouse(mouse_event) = &event
            && let Some(pos) = cursor.position()
        {
            const ANNOTATION_MARGIN: f32 = 0.0;

            let clamp_to_selection =
                |x: f32, y: f32, sel_x: f32, sel_y: f32, sel_w: f32, sel_h: f32| -> (f32, f32) {
                    let min_x = sel_x + ANNOTATION_MARGIN;
                    let max_x = sel_x + sel_w - ANNOTATION_MARGIN;
                    let min_y = sel_y + ANNOTATION_MARGIN;
                    let max_y = sel_y + sel_h - ANNOTATION_MARGIN;
                    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
                };

            let inside_inner_selection = |sel_x: f32, sel_y: f32, sel_w: f32, sel_h: f32| -> bool {
                pos.x >= sel_x + ANNOTATION_MARGIN
                    && pos.x <= sel_x + sel_w - ANNOTATION_MARGIN
                    && pos.y >= sel_y + ANNOTATION_MARGIN
                    && pos.y <= sel_y + sel_h - ANNOTATION_MARGIN
            };

            // Handle arrow drawing
            if self.is_arrow_mode() {
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        inside_inner_selection(sel_x, sel_y, sel_w, sel_h)
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell.publish(
                                self.emit(ScreenshotEvent::arrow_start(global_x, global_y)),
                            );
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    MouseEvent::ButtonReleased(Button::Left)
                        if self.annotations.arrow_drawing.is_some() =>
                    {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell
                                .publish(self.emit(ScreenshotEvent::arrow_end(global_x, global_y)));
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    _ => {}
                }
            }

            // Handle redact drawing
            if self.is_redact_mode() {
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        inside_inner_selection(sel_x, sel_y, sel_w, sel_h)
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell.publish(
                                self.emit(ScreenshotEvent::redact_start(global_x, global_y)),
                            );
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    MouseEvent::ButtonReleased(Button::Left)
                        if self.annotations.redact_drawing.is_some() =>
                    {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell.publish(
                                self.emit(ScreenshotEvent::redact_end(global_x, global_y)),
                            );
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    _ => {}
                }
            }

            // Handle pixelate drawing
            if self.is_pixelate_mode() {
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        inside_inner_selection(sel_x, sel_y, sel_w, sel_h)
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell.publish(
                                self.emit(ScreenshotEvent::pixelate_start(global_x, global_y)),
                            );
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    MouseEvent::ButtonReleased(Button::Left)
                        if self.annotations.pixelate_drawing.is_some() =>
                    {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell.publish(
                                self.emit(ScreenshotEvent::pixelate_end(global_x, global_y)),
                            );
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    _ => {}
                }
            }
        }

        status
    }

    fn mouse_interaction(
        &self,
        state: &Tree,
        layout: Layout<'_>,
        cursor: cosmic::iced::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
        renderer: &cosmic::Renderer,
    ) -> cosmic::iced::mouse::Interaction {
        if self.is_any_drawing_mode() && cursor.position().is_some() {
            return cosmic::iced::mouse::Interaction::Crosshair;
        }

        let mut children: Vec<&Element<'_, Msg>> = vec![
            &self.bg_element,
            &self.fg_element,
            &self.shapes_element,
            &self.menu_element,
        ];
        if let Some(ref drawer) = self.settings_drawer_element {
            children.push(drawer);
        }
        if let Some(ref selector) = self.shape_popup_element {
            children.push(selector);
        }
        if let Some(ref popup) = self.redact_popup_element {
            children.push(popup);
        }

        let layout = layout.children().collect::<Vec<_>>();
        for (i, (layout, child)) in layout
            .into_iter()
            .zip(children.into_iter())
            .enumerate()
            .rev()
        {
            let tree = &state.children[i];
            let interaction = child
                .as_widget()
                .mouse_interaction(tree, layout, cursor, viewport, renderer);
            if cursor.is_over(layout.bounds()) {
                return interaction;
            }
        }
        cosmic::iced::mouse::Interaction::default()
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        translation: iced::Vector,
    ) -> Option<overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>> {
        let mut elements: Vec<&mut Element<'_, Msg>> = vec![
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.shapes_element,
            &mut self.menu_element,
        ];
        if let Some(ref mut drawer) = self.settings_drawer_element {
            elements.push(drawer);
        }
        if let Some(ref mut selector) = self.shape_popup_element {
            elements.push(selector);
        }
        if let Some(ref mut popup) = self.redact_popup_element {
            elements.push(popup);
        }

        let children = elements
            .into_iter()
            .zip(&mut state.children)
            .zip(layout.children())
            .filter_map(|((child, state), layout)| {
                child
                    .as_widget_mut()
                    .overlay(state, layout, renderer, translation)
            })
            .collect::<Vec<_>>();

        (!children.is_empty()).then(|| overlay::Group::with_children(children).overlay())
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn cosmic::widget::Operation<()>,
    ) {
        let layout = layout.children().collect::<Vec<_>>();
        let mut children: Vec<&Element<'_, Msg>> = vec![
            &self.bg_element,
            &self.fg_element,
            &self.shapes_element,
            &self.menu_element,
        ];
        if let Some(ref drawer) = self.settings_drawer_element {
            children.push(drawer);
        }
        if let Some(ref selector) = self.shape_popup_element {
            children.push(selector);
        }
        if let Some(ref popup) = self.redact_popup_element {
            children.push(popup);
        }
        for (i, (layout, child)) in layout
            .into_iter()
            .zip(children.into_iter())
            .enumerate()
            .rev()
        {
            let tree = &mut tree.children[i];
            child.as_widget().operate(tree, layout, renderer, operation);
        }
    }

    fn tag(&self) -> cosmic::iced_core::widget::tree::Tag {
        cosmic::iced_core::widget::tree::Tag::of::<()>()
    }

    fn state(&self) -> cosmic::iced_core::widget::tree::State {
        cosmic::iced_core::widget::tree::State::None
    }

    fn id(&self) -> Option<cosmic::widget::Id> {
        Some(self.id.clone())
    }

    fn set_id(&mut self, id: cosmic::widget::Id) {
        self.id = id;
    }

    fn drag_destinations(
        &self,
        state: &Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        dnd_rectangles: &mut cosmic::iced_core::clipboard::DndDestinationRectangles,
    ) {
        let mut children: Vec<&Element<'_, Msg>> =
            vec![&self.bg_element, &self.fg_element, &self.menu_element];
        if let Some(ref drawer) = self.settings_drawer_element {
            children.push(drawer);
        }
        if let Some(ref selector) = self.shape_popup_element {
            children.push(selector);
        }
        if let Some(ref popup) = self.redact_popup_element {
            children.push(popup);
        }
        for (i, (layout, child)) in layout.children().zip(children).enumerate() {
            let state = &state.children[i];
            child
                .as_widget()
                .drag_destinations(state, layout, renderer, dnd_rectangles);
        }
    }
}

impl<'a, E> From<ScreenshotSelectionWidget<'a, E>> for Element<'a, Msg>
where
    E: Fn(ScreenshotEvent) -> Msg + Clone + 'static,
{
    fn from(widget: ScreenshotSelectionWidget<'a, E>) -> Self {
        Self::new(widget)
    }
}

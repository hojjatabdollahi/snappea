use std::{borrow::Cow, collections::HashMap, rc::Rc};

use cosmic::{
    Element,
    cosmic_theme::Spacing,
    iced::{self, window},
    iced_core::{
        Background, Border, ContentFit, Degrees, Layout, Length, Point, Size, alignment,
        gradient::Linear, layout, overlay, widget::Tree,
    },
    iced_widget::{
        column, row,
        graphics::{
            Mesh,
            color::pack,
            mesh::{Indexed, Renderer as MeshRenderer, SolidVertex2D},
        },
    },
    widget::{
        Row, button, divider::vertical, horizontal_space, icon, image, layer_container,
    },
};
use cosmic_bg_config::Source;
use wayland_client::protocol::wl_output::WlOutput;

use crate::{
    app::OutputState,
    screenshot::{ArrowAnnotation, Choice, DetectedQrCode, OcrStatus, OcrTextOverlay, Rect, ScreenshotImage, ToolbarPosition},
};

use super::{
    output_selection::OutputSelection,
    rectangle_selection::{DragState, RectangleSelection},
};

/// Widget for selecting toolbar position with triangular hit regions
pub struct ToolbarPositionSelector<Msg> {
    size: f32,
    current_position: ToolbarPosition,
    on_top: Msg,
    on_bottom: Msg,
    on_left: Msg,
    on_right: Msg,
}

impl<Msg: Clone> ToolbarPositionSelector<Msg> {
    pub fn new(
        size: f32,
        current_position: ToolbarPosition,
        on_top: Msg,
        on_bottom: Msg,
        on_left: Msg,
        on_right: Msg,
    ) -> Self {
        Self {
            size,
            current_position,
            on_top,
            on_bottom,
            on_left,
            on_right,
        }
    }

    /// Determine which triangular region a point falls into
    /// The square is divided into 4 triangles from the center
    /// Extends clickable area slightly beyond visual bounds
    fn get_region(&self, x: f32, y: f32, bounds: cosmic::iced_core::Rectangle) -> Option<ToolbarPosition> {
        // Extend the clickable region by a margin
        let margin = 8.0;
        let extended_bounds = cosmic::iced_core::Rectangle {
            x: bounds.x - margin,
            y: bounds.y - margin,
            width: bounds.width + margin * 2.0,
            height: bounds.height + margin * 2.0,
        };
        
        let local_x = x - extended_bounds.x;
        let local_y = y - extended_bounds.y;
        
        // Check if point is inside extended bounds
        if local_x < 0.0 || local_x > extended_bounds.width || local_y < 0.0 || local_y > extended_bounds.height {
            return None;
        }
        
        // Calculate which triangle the point is in
        // Top triangle: above both diagonals
        // Bottom triangle: below both diagonals
        // Left triangle: left of both diagonals
        // Right triangle: right of both diagonals
        
        // Diagonal from top-left to bottom-right: y = x * (height/width)
        // Diagonal from top-right to bottom-left: y = height - x * (height/width)
        
        let diag1 = local_x * (extended_bounds.height / extended_bounds.width); // TL to BR
        let diag2 = extended_bounds.height - local_x * (extended_bounds.height / extended_bounds.width); // TR to BL
        
        let above_diag1 = local_y < diag1;
        let above_diag2 = local_y < diag2;
        
        match (above_diag1, above_diag2) {
            (true, true) => Some(ToolbarPosition::Top),    // Above both diagonals
            (false, false) => Some(ToolbarPosition::Bottom), // Below both diagonals
            (true, false) => Some(ToolbarPosition::Right),  // Above diag1, below diag2
            (false, true) => Some(ToolbarPosition::Left),   // Below diag1, above diag2
        }
    }
}

impl<Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer> for ToolbarPositionSelector<Msg> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(&self, _tree: &mut cosmic::iced_core::widget::Tree, _renderer: &cosmic::Renderer, _limits: &cosmic::iced::Limits) -> layout::Node {
        layout::Node::new(Size::new(self.size, self.size))
    }

    fn draw(
        &self,
        _tree: &cosmic::iced_core::widget::Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        _style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        _viewport: &cosmic::iced_core::Rectangle,
    ) {
        use cosmic::iced_core::Renderer as _;
        
        let bounds = layout.bounds();
        let cosmic_theme = theme.cosmic();
        let accent = cosmic::iced::Color::from(cosmic_theme.accent_color());
        let radius = cosmic_theme.radius_xs();
        
        // Use more visible colors
        let base_color = cosmic::iced::Color::from_rgba(0.4, 0.4, 0.4, 0.6);
        let hover_color = cosmic::iced::Color::from_rgba(0.6, 0.6, 0.6, 0.8);
        
        // Determine hovered region
        let hovered_region = cursor.position().and_then(|pos| self.get_region(pos.x, pos.y, bounds));
        
        let edge_thickness = 6.0;
        let gap = 3.0;
        let inner_length = bounds.width - edge_thickness * 2.0 - gap * 2.0;
        
        // Draw the 4 edge rectangles with borders
        // Top edge
        let top_color = if self.current_position == ToolbarPosition::Top {
            accent
        } else if hovered_region == Some(ToolbarPosition::Top) {
            hover_color
        } else {
            base_color
        };
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: cosmic::iced_core::Rectangle {
                    x: bounds.x + edge_thickness + gap,
                    y: bounds.y,
                    width: inner_length,
                    height: edge_thickness,
                },
                border: Border {
                    radius: radius.into(),
                    width: 1.0,
                    color: accent,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(top_color),
        );
        
        // Bottom edge
        let bottom_color = if self.current_position == ToolbarPosition::Bottom {
            accent
        } else if hovered_region == Some(ToolbarPosition::Bottom) {
            hover_color
        } else {
            base_color
        };
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: cosmic::iced_core::Rectangle {
                    x: bounds.x + edge_thickness + gap,
                    y: bounds.y + bounds.height - edge_thickness,
                    width: inner_length,
                    height: edge_thickness,
                },
                border: Border {
                    radius: radius.into(),
                    width: 1.0,
                    color: accent,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(bottom_color),
        );
        
        // Left edge
        let left_color = if self.current_position == ToolbarPosition::Left {
            accent
        } else if hovered_region == Some(ToolbarPosition::Left) {
            hover_color
        } else {
            base_color
        };
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: cosmic::iced_core::Rectangle {
                    x: bounds.x,
                    y: bounds.y + edge_thickness + gap,
                    width: edge_thickness,
                    height: inner_length,
                },
                border: Border {
                    radius: radius.into(),
                    width: 1.0,
                    color: accent,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(left_color),
        );
        
        // Right edge
        let right_color = if self.current_position == ToolbarPosition::Right {
            accent
        } else if hovered_region == Some(ToolbarPosition::Right) {
            hover_color
        } else {
            base_color
        };
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: cosmic::iced_core::Rectangle {
                    x: bounds.x + bounds.width - edge_thickness,
                    y: bounds.y + edge_thickness + gap,
                    width: edge_thickness,
                    height: inner_length,
                },
                border: Border {
                    radius: radius.into(),
                    width: 1.0,
                    color: accent,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(right_color),
        );
    }

    fn mouse_interaction(
        &self,
        _state: &cosmic::iced_core::widget::Tree,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        _viewport: &cosmic::iced_core::Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> cosmic::iced_core::mouse::Interaction {
        if let Some(pos) = cursor.position() {
            if self.get_region(pos.x, pos.y, layout.bounds()).is_some() {
                return cosmic::iced_core::mouse::Interaction::Pointer;
            }
        }
        cosmic::iced_core::mouse::Interaction::default()
    }

    fn on_event(
        &mut self,
        _state: &mut cosmic::iced_core::widget::Tree,
        event: cosmic::iced_core::Event,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        _renderer: &cosmic::Renderer,
        _clipboard: &mut dyn cosmic::iced_core::Clipboard,
        shell: &mut cosmic::iced_core::Shell<'_, Msg>,
        _viewport: &cosmic::iced_core::Rectangle,
    ) -> cosmic::iced_core::event::Status {
        if let cosmic::iced_core::Event::Mouse(cosmic::iced_core::mouse::Event::ButtonPressed(
            cosmic::iced_core::mouse::Button::Left,
        )) = event
        {
            if let Some(pos) = cursor.position() {
                if let Some(region) = self.get_region(pos.x, pos.y, layout.bounds()) {
                    let msg = match region {
                        ToolbarPosition::Top => self.on_top.clone(),
                        ToolbarPosition::Bottom => self.on_bottom.clone(),
                        ToolbarPosition::Left => self.on_left.clone(),
                        ToolbarPosition::Right => self.on_right.clone(),
                    };
                    shell.publish(msg);
                    return cosmic::iced_core::event::Status::Captured;
                }
            }
        }
        cosmic::iced_core::event::Status::Ignored
    }
}

impl<'a, Msg: Clone + 'static> From<ToolbarPositionSelector<Msg>> for Element<'a, Msg> {
    fn from(widget: ToolbarPositionSelector<Msg>) -> Self {
        Element::new(widget)
    }
}

/// Widget for displaying a selected window with a border (buttons are in toolbar)
pub struct SelectedImageWidget {
    image_handle: Option<cosmic::widget::image::Handle>,
    image_size: (u32, u32),
}

impl SelectedImageWidget {
    pub fn new(
        output_name: String,
        window_index: Option<usize>,
        toplevel_images: &HashMap<String, Vec<ScreenshotImage>>,
    ) -> Self {
        let (image_handle, image_size) = if let Some(window_index) = window_index {
            toplevel_images
                .get(&output_name)
                .and_then(|imgs| imgs.get(window_index))
                .map(|img| (Some(img.handle.clone()), (img.rgba.width(), img.rgba.height())))
                .unwrap_or((None, (0, 0)))
        } else {
            (None, (0, 0))
        };
        
        Self {
            image_handle,
            image_size,
        }
    }

    /// Calculate the bounds where the image should be drawn (centered in the output)
    pub fn image_bounds(&self, layout_bounds: cosmic::iced_core::Rectangle) -> cosmic::iced_core::Rectangle {
        if self.image_handle.is_some() && self.image_size.0 > 0 {
            let img_width = self.image_size.0 as f32;
            let img_height = self.image_size.1 as f32;
            
            // Leave small margin around the image
            let available_width = layout_bounds.width - 20.0;
            let available_height = layout_bounds.height - 20.0;
            
            // Calculate scale to fit image within available space
            let scale_x = available_width / img_width;
            let scale_y = available_height / img_height;
            let scale = scale_x.min(scale_y).min(1.0); // Don't upscale
            
            let display_width = img_width * scale;
            let display_height = img_height * scale;
            
            cosmic::iced_core::Rectangle {
                x: layout_bounds.x + (layout_bounds.width - display_width) / 2.0,
                y: layout_bounds.y + (layout_bounds.height - display_height) / 2.0,
                width: display_width,
                height: display_height,
            }
        } else {
            layout_bounds
        }
    }
}

impl<Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer> for SelectedImageWidget {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&self, _tree: &mut Tree, _renderer: &cosmic::Renderer, limits: &cosmic::iced::Limits) -> layout::Node {
        let limits = limits.width(Length::Fill).height(Length::Fill);
        layout::Node::new(limits.resolve(Length::Fill, Length::Fill, Size::ZERO))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        _style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        _cursor: cosmic::iced_core::mouse::Cursor,
        _viewport: &cosmic::iced_core::Rectangle,
    ) {
        use cosmic::iced_core::Renderer as _;
        
        let cosmic_theme = theme.cosmic();
        let layout_bounds = layout.bounds();
        let image_bounds = self.image_bounds(layout_bounds);
        
        // Draw the image
        if let Some(ref handle) = self.image_handle {
            cosmic::iced_core::image::Renderer::draw_image(
                renderer,
                handle.clone(),
                cosmic::iced_core::image::FilterMethod::Linear,
                image_bounds,
                cosmic::iced::Radians(0.0),
                1.0,
                [0.0, 0.0, 0.0, 0.0],
            );
        }
        
        // Draw border around the image
        let accent = cosmic::iced::Color::from(cosmic_theme.accent_color());
        
        // Semi-transparent glow
        let mut glow_color = accent;
        glow_color.a = 0.5;
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: cosmic::iced_core::Rectangle {
                    x: image_bounds.x - 2.0,
                    y: image_bounds.y - 2.0,
                    width: image_bounds.width + 4.0,
                    height: image_bounds.height + 4.0,
                },
                border: Border {
                    radius: 0.0.into(),
                    width: 6.0,
                    color: glow_color,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(cosmic::iced::Color::TRANSPARENT),
        );
        
        // Solid accent border
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: image_bounds,
                border: Border {
                    radius: 0.0.into(),
                    width: 2.0,
                    color: accent,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(cosmic::iced::Color::TRANSPARENT),
        );
        
        // Corner handles
        let corner_size = 12.0;
        let corners = [
            (image_bounds.x, image_bounds.y),
            (image_bounds.x + image_bounds.width, image_bounds.y),
            (image_bounds.x, image_bounds.y + image_bounds.height),
            (image_bounds.x + image_bounds.width, image_bounds.y + image_bounds.height),
        ];
        for (cx, cy) in corners {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: cx - corner_size / 2.0,
                        y: cy - corner_size / 2.0,
                        width: corner_size,
                        height: corner_size,
                    },
                    border: Border {
                        radius: cosmic_theme.radius_s().into(),
                        width: 0.0,
                        color: cosmic::iced::Color::TRANSPARENT,
                    },
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                Background::Color(accent),
            );
        }
    }
}

impl<'a, Msg: Clone + 'static> From<SelectedImageWidget> for Element<'a, Msg> {
    fn from(widget: SelectedImageWidget) -> Self {
        Element::new(widget)
    }
}

pub struct ScreenshotSelection<'a, Msg> {
    id: cosmic::widget::Id,
    pub choice: Choice,
    pub choices: Vec<Choice>,
    pub output_logical_geo: Vec<Rect>,
    pub choice_labels: Vec<Cow<'a, str>>,
    pub bg_element: Element<'a, Msg>,
    pub fg_element: Element<'a, Msg>,
    pub menu_element: Element<'a, Msg>,
    /// QR codes to display, with their positions relative to this output
    pub qr_codes: Vec<(f32, f32, String)>, // (x, y, content)
    /// OCR overlays to display (bounding box + block_num for coloring)
    pub ocr_overlays: Vec<(f32, f32, f32, f32, i32)>, // (left, top, width, height, block_num)
    /// Selection rectangle bounds (output-relative) for constraining overlays
    pub selection_rect: Option<(f32, f32, f32, f32)>, // (x, y, width, height)
    /// Whether to show QR overlays (hidden when dragging)
    pub show_qr_overlays: bool,
    /// Whether QR scanning is in progress
    pub qr_scanning: bool,
    /// OCR status for display
    pub ocr_status: OcrStatus,
    /// Output rect for this widget
    pub output_rect: Rect,
    /// Output name for this widget
    pub output_name: String,
    /// Arrow annotations
    pub arrows: Vec<ArrowAnnotation>,
    /// Whether arrow mode is active
    pub arrow_mode: bool,
    /// Arrow currently being drawn (start point)
    pub arrow_drawing: Option<(f32, f32)>,
    /// Callbacks for arrow mode
    pub on_arrow_toggle: Option<Msg>,
    pub on_arrow_start: Option<Box<dyn Fn(f32, f32) -> Msg + 'a>>,
    pub on_arrow_end: Option<Box<dyn Fn(f32, f32) -> Msg + 'a>>,
    /// Toolbar position
    pub toolbar_position: ToolbarPosition,
    /// Callback for toolbar position change
    pub on_toolbar_position: Option<Box<dyn Fn(ToolbarPosition) -> Msg + 'a>>,
}

impl<'a, Msg> ScreenshotSelection<'a, Msg>
where
    Msg: 'static + Clone,
{
    pub fn new(
        choice: Choice,
        image: &'a ScreenshotImage,
        on_copy_to_clipboard: Msg,
        on_save_to_pictures: Msg,
        on_cancel: Msg,
        on_ocr: Msg,
        on_ocr_copy: Msg,
        on_qr: Msg,
        on_qr_copy: Msg,
        output: &OutputState,
        window_id: window::Id,
        on_output_change: impl Fn(WlOutput) -> Msg,
        on_choice_change: impl Fn(Choice) -> Msg + 'static + Clone,
        toplevel_images: &HashMap<String, Vec<ScreenshotImage>>,
        toplevel_chosen: impl Fn(String, usize) -> Msg,
        spacing: Spacing,
        dnd_id: u128,
        qr_codes: &[DetectedQrCode],
        qr_scanning: bool,
        ocr_overlays: &[OcrTextOverlay],
        ocr_status: OcrStatus,
        has_ocr_text: bool,
        arrows: &[ArrowAnnotation],
        arrow_mode: bool,
        arrow_drawing: Option<(f32, f32)>,
        on_arrow_toggle: Msg,
        on_arrow_start: impl Fn(f32, f32) -> Msg + 'a,
        on_arrow_end: impl Fn(f32, f32) -> Msg + 'a,
        toolbar_position: ToolbarPosition,
        on_toolbar_position: impl Fn(ToolbarPosition) -> Msg + 'a,
    ) -> Self {
        let space_l = spacing.space_l;
        let space_s = spacing.space_s;
        let space_xs = spacing.space_xs;
        let space_xxs = spacing.space_xxs;

        let output_rect = Rect {
            left: output.logical_pos.0,
            top: output.logical_pos.1,
            right: output.logical_pos.0 + output.logical_size.0 as i32,
            bottom: output.logical_pos.1 + output.logical_size.1 as i32,
        };

        let on_choice_change_clone = on_choice_change.clone();
        // Calculate scale factor (physical pixels per logical pixel)
        let image_scale = image.rgba.width() as f32 / output.logical_size.0 as f32;
        
        let fg_element = match choice {
            Choice::Rectangle(r, drag_state) => RectangleSelection::new(
                output_rect,
                r,
                drag_state,
                window_id,
                dnd_id,
                move |s, r| on_choice_change_clone(Choice::Rectangle(r, s)),
                &image.rgba,
                image_scale,
                arrow_mode,
            )
            .into(),
            Choice::Output(_) => {
                OutputSelection::new(on_output_change(output.output.clone())).into()
            }
            Choice::Window(ref win_output, None) => {
                // Window picker mode - show all windows as buttons
                let imgs = toplevel_images
                    .get(win_output)
                    .map(|x| x.as_slice())
                    .unwrap_or_default();
                let total_img_width = imgs.iter().map(|img| img.width()).sum::<u32>();

                let img_buttons = imgs.iter().enumerate().map(|(i, img)| {
                    let portion =
                        (img.width() as u64 * u16::MAX as u64 / total_img_width as u64).max(1);
                    layer_container(
                        button::custom(
                            image::Image::new(img.handle.clone())
                                .content_fit(ContentFit::ScaleDown),
                        )
                        .on_press(toplevel_chosen(output.name.clone(), i))
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
            Choice::Window(ref win_output, Some(win_index)) => {
                // Selected window mode - show the window image with border (buttons are in toolbar)
                SelectedImageWidget::new(
                    win_output.clone(),
                    Some(win_index),
                    toplevel_images,
                )
                .into()
            }
        };

        let bg_element = match choice {
            Choice::Output(_) | Choice::Rectangle(..) | Choice::Window(_, Some(_)) => {
                // For rectangle, output, and selected window modes, show the screenshot
                image::Image::new(image.handle.clone())
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
        let active_icon =
            cosmic::theme::Svg::Custom(Rc::new(|t| cosmic::iced_widget::svg::Style {
                color: Some(t.cosmic().accent_color().into()),
            }));
        
        // Build QR overlay - only show when not actively dragging a rectangle
        let show_qr_overlays = match choice {
            Choice::Rectangle(_, DragState::None) => true,
            Choice::Rectangle(_, _) => false, // Hide when dragging
            _ => true,
        };
        
        // Filter and prepare QR codes for this output
        let qr_codes_for_output: Vec<(f32, f32, String)> = qr_codes
            .iter()
            .filter(|qr| qr.output_name == output.name)
            .map(|qr| (qr.center_x, qr.center_y, qr.content.clone()))
            .collect();
        log::debug!("Widget received {} OCR overlays, filtering for output '{}'", ocr_overlays.len(), output.name);
        let ocr_overlays_for_output: Vec<(f32, f32, f32, f32, i32)> = ocr_overlays
            .iter()
            .filter(|o| {
                let matches = o.output_name == output.name;
                log::debug!("  Overlay output '{}' matches: {}", o.output_name, matches);
                matches
            })
            .map(|o| (o.left, o.top, o.width, o.height, o.block_num))
            .collect();
        log::debug!("After filtering: {} OCR overlays for this output", ocr_overlays_for_output.len());
        
        // Calculate selection rectangle relative to this output
        let selection_rect = match &choice {
            Choice::Rectangle(r, _) => {
                if let Some(intersection) = r.intersect(output_rect) {
                    let x = (intersection.left - output_rect.left) as f32;
                    let y = (intersection.top - output_rect.top) as f32;
                    let w = intersection.width() as f32;
                    let h = intersection.height() as f32;
                    if w > 0.0 && h > 0.0 {
                        Some((x, y, w, h))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Choice::Window(win_output, Some(win_idx)) => {
                // For selected window mode, calculate where the window image will be drawn (centered)
                if let Some(img) = toplevel_images.get(win_output).and_then(|imgs| imgs.get(*win_idx)) {
                    let img_width = img.rgba.width() as f32;
                    let img_height = img.rgba.height() as f32;
                    let output_width = output.logical_size.0 as f32;
                    let output_height = output.logical_size.1 as f32;
                    
                    // Match the centering logic in SelectedImageWidget::image_bounds (20px margin)
                    let available_width = output_width - 20.0;
                    let available_height = output_height - 20.0;
                    let scale_x = available_width / img_width;
                    let scale_y = available_height / img_height;
                    let scale = scale_x.min(scale_y).min(1.0);
                    
                    let display_width = img_width * scale;
                    let display_height = img_height * scale;
                    let x = (output_width - display_width) / 2.0;
                    let y = (output_height - display_height) / 2.0;
                    
                    Some((x, y, display_width, display_height))
                } else {
                    None
                }
            }
            Choice::Output(_) => {
                // For output mode, the entire output is the selection area
                Some((0.0, 0.0, output.logical_size.0 as f32, output.logical_size.1 as f32))
            }
            _ => None,
        };
        
        Self {
            id: cosmic::widget::Id::unique(),
            choices: Vec::new(),
            output_logical_geo: Vec::new(),
            choice_labels: Vec::new(),
            bg_element,
            fg_element,
            qr_codes: qr_codes_for_output,
            ocr_overlays: ocr_overlays_for_output,
            selection_rect,
            show_qr_overlays,
            qr_scanning,
            ocr_status,
            menu_element: {
                // Position selector - custom widget with triangular hit regions
                let position_selector: Element<'_, Msg> = ToolbarPositionSelector::new(
                    40.0, // size of the selector widget
                    toolbar_position,
                    on_toolbar_position(ToolbarPosition::Top),
                    on_toolbar_position(ToolbarPosition::Bottom),
                    on_toolbar_position(ToolbarPosition::Left),
                    on_toolbar_position(ToolbarPosition::Right),
                ).into();
                
                // Common buttons
                let btn_region = button::custom(
                    icon::Icon::from(icon::from_name("screenshot-selection-symbolic").size(64))
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                        .class(if matches!(choice, Choice::Rectangle(..)) { active_icon.clone() } else { cosmic::theme::Svg::default() })
                )
                .selected(matches!(choice, Choice::Rectangle(..)))
                .class(cosmic::theme::Button::Icon)
                .on_press(on_choice_change(Choice::Rectangle(Rect::default(), DragState::None)))
                .padding(space_xs);
                
                let btn_window = button::custom(
                    icon::Icon::from(icon::from_name("screenshot-window-symbolic").size(64))
                        .class(if matches!(choice, Choice::Window(..)) { active_icon.clone() } else { cosmic::theme::Svg::default() })
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                )
                .selected(matches!(choice, Choice::Window(..)))
                .class(cosmic::theme::Button::Icon)
                .on_press(on_choice_change(Choice::Window(output.name.clone(), None)))
                .padding(space_xs);
                
                let btn_screen = button::custom(
                    icon::Icon::from(icon::from_name("screenshot-screen-symbolic").size(64))
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                        .class(if matches!(choice, Choice::Output(..)) { active_icon.clone() } else { cosmic::theme::Svg::default() })
                )
                .selected(matches!(choice, Choice::Output(..)))
                .class(cosmic::theme::Button::Icon)
                .on_press(on_choice_change(Choice::Output(output.name.clone())))
                .padding(space_xs);
                
                // Check if a selection is complete (can show action buttons)
                let has_selection = match choice {
                    Choice::Rectangle(r, _) => r.dimensions().is_some(),
                    Choice::Window(_, Some(_)) => true,
                    Choice::Output(_) => true,
                    _ => false,
                };
                
                // Copy to clipboard button
                let btn_copy = button::custom(
                    icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                )
                .class(cosmic::theme::Button::Icon)
                .on_press_maybe(has_selection.then_some(on_copy_to_clipboard))
                .padding(space_xs);
                
                // Save to pictures button
                let btn_save = button::custom(
                    icon::Icon::from(icon::from_name("document-save-symbolic").size(64))
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                )
                .class(cosmic::theme::Button::Icon)
                .on_press_maybe(has_selection.then_some(on_save_to_pictures))
                .padding(space_xs);
                
                // Arrow drawing button
                let btn_arrow = button::custom(
                    icon::Icon::from(icon::from_name("arrow-symbolic").size(64))
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                )
                .class(if arrow_mode { cosmic::theme::Button::Suggested } else { cosmic::theme::Button::Icon })
                .on_press_maybe(has_selection.then_some(on_arrow_toggle.clone()))
                .padding(space_xs);
                
                // OCR button
                let btn_ocr = if has_ocr_text {
                    button::custom(
                        icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                            .width(Length::Fixed(40.0))
                            .height(Length::Fixed(40.0))
                    )
                    .class(cosmic::theme::Button::Suggested)
                    .on_press_maybe(has_selection.then_some(on_ocr_copy.clone()))
                    .padding(space_xs)
                } else {
                    button::custom(
                        icon::Icon::from(icon::from_name("ocr-symbolic").size(64))
                            .width(Length::Fixed(40.0))
                            .height(Length::Fixed(40.0))
                    )
                    .class(cosmic::theme::Button::Icon)
                    .on_press_maybe(has_selection.then_some(on_ocr.clone()))
                    .padding(space_xs)
                };
                
                // QR button
                let has_qr_codes = !qr_codes.is_empty();
                let btn_qr = if has_qr_codes {
                    button::custom(
                        icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                            .width(Length::Fixed(40.0))
                            .height(Length::Fixed(40.0))
                    )
                    .class(cosmic::theme::Button::Suggested)
                    .on_press_maybe(has_selection.then_some(on_qr_copy.clone()))
                    .padding(space_xs)
                } else {
                    button::custom(
                        icon::Icon::from(icon::from_name("qr-symbolic").size(64))
                            .width(Length::Fixed(40.0))
                            .height(Length::Fixed(40.0))
                    )
                    .class(cosmic::theme::Button::Icon)
                    .on_press_maybe(has_selection.then_some(on_qr.clone()))
                    .padding(space_xs)
                };
                
                let btn_close = button::custom(
                    icon::Icon::from(icon::from_name("window-close-symbolic").size(63))
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                )
                .class(cosmic::theme::Button::Icon)
                .on_press(on_cancel);
                
                let is_vertical = matches!(toolbar_position, ToolbarPosition::Left | ToolbarPosition::Right);
                
                let toolbar_content: Element<'_, Msg> = if is_vertical {
                    // Vertical layout for left/right positions
                    use cosmic::widget::divider::horizontal;
                    if has_selection {
                        column![
                            position_selector,
                            horizontal::light().width(Length::Fixed(64.0)),
                            column![btn_region, btn_window, btn_screen].spacing(space_s).align_x(cosmic::iced_core::Alignment::Center),
                            horizontal::light().width(Length::Fixed(64.0)),
                            column![btn_arrow, btn_ocr, btn_qr].spacing(space_s).align_x(cosmic::iced_core::Alignment::Center),
                            horizontal::light().width(Length::Fixed(64.0)),
                            column![btn_copy, btn_save].spacing(space_s).align_x(cosmic::iced_core::Alignment::Center),
                            horizontal::light().width(Length::Fixed(64.0)),
                            btn_close,
                        ]
                        .align_x(cosmic::iced_core::Alignment::Center)
                        .spacing(space_s)
                        .padding([space_s, space_xxs, space_s, space_xxs])
                        .into()
                    } else {
                        column![
                            position_selector,
                            horizontal::light().width(Length::Fixed(64.0)),
                            column![btn_region, btn_window, btn_screen].spacing(space_s).align_x(cosmic::iced_core::Alignment::Center),
                            horizontal::light().width(Length::Fixed(64.0)),
                            btn_close,
                        ]
                        .align_x(cosmic::iced_core::Alignment::Center)
                        .spacing(space_s)
                        .padding([space_s, space_xxs, space_s, space_xxs])
                        .into()
                    }
                } else {
                    // Horizontal layout for top/bottom positions
                    if has_selection {
                        row![
                            position_selector,
                            vertical::light().height(Length::Fixed(64.0)),
                            row![btn_region, btn_window, btn_screen].spacing(space_s).align_y(cosmic::iced_core::Alignment::Center),
                            vertical::light().height(Length::Fixed(64.0)),
                            row![btn_arrow, btn_ocr, btn_qr].spacing(space_s).align_y(cosmic::iced_core::Alignment::Center),
                            vertical::light().height(Length::Fixed(64.0)),
                            row![btn_copy, btn_save].spacing(space_s).align_y(cosmic::iced_core::Alignment::Center),
                            vertical::light().height(Length::Fixed(64.0)),
                            btn_close,
                        ]
                        .align_y(cosmic::iced_core::Alignment::Center)
                        .spacing(space_s)
                        .padding([space_xxs, space_s, space_xxs, space_s])
                        .into()
                    } else {
                        row![
                            position_selector,
                            vertical::light().height(Length::Fixed(64.0)),
                            row![btn_region, btn_window, btn_screen].spacing(space_s).align_y(cosmic::iced_core::Alignment::Center),
                            vertical::light().height(Length::Fixed(64.0)),
                            btn_close,
                        ]
                        .align_y(cosmic::iced_core::Alignment::Center)
                        .spacing(space_s)
                        .padding([space_xxs, space_s, space_xxs, space_s])
                        .into()
                    }
                };
                
                cosmic::widget::container(toolbar_content)
            }
            .class(cosmic::theme::Container::Custom(Box::new(|theme| {
                let theme = theme.cosmic();
                cosmic::iced::widget::container::Style {
                    background: Some(Background::Color(theme.background.component.base.into())),
                    text_color: Some(theme.background.component.on.into()),
                    border: Border {
                        radius: theme.corner_radii.radius_s.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })))
            .into(),
            choice,
            output_rect,
            output_name: output.name.clone(),
            arrows: arrows.to_vec(),
            arrow_mode,
            arrow_drawing,
            on_arrow_toggle: Some(on_arrow_toggle),
            on_arrow_start: Some(Box::new(on_arrow_start)),
            on_arrow_end: Some(Box::new(on_arrow_end)),
            toolbar_position,
            on_toolbar_position: Some(Box::new(on_toolbar_position)),
        }
    }
}

impl<'a, Msg: Clone> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for ScreenshotSelection<'a, Msg>
{
    fn children(&self) -> Vec<cosmic::iced_core::widget::Tree> {
        vec![
            Tree::new(&self.bg_element),
            Tree::new(&self.fg_element),
            Tree::new(&self.menu_element),
        ]
    }

    fn diff(&mut self, tree: &mut cosmic::iced_core::widget::Tree) {
        tree.diff_children(&mut [
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.menu_element,
        ]);
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        translation: iced::Vector,
    ) -> Option<cosmic::iced_core::overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>> {
        let children = [
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.menu_element,
        ]
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

    fn on_event(
        &mut self,
        tree: &mut cosmic::iced_core::widget::Tree,
        event: cosmic::iced_core::Event,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn cosmic::iced_core::Clipboard,
        shell: &mut cosmic::iced_core::Shell<'_, Msg>,
        viewport: &cosmic::iced_core::Rectangle,
    ) -> cosmic::iced_core::event::Status {
        use cosmic::iced_core::mouse::{Button, Event as MouseEvent};
        
        // First, let child widgets handle the event (this includes toolbar buttons)
        let children = [
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.menu_element,
        ];

        let layout_children = layout.children().collect::<Vec<_>>();
        let mut status = cosmic::iced_core::event::Status::Ignored;
        for (i, (child_layout, child)) in layout_children
            .into_iter()
            .zip(children.into_iter())
            .enumerate()
            .rev()
        {
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
        
        // If child widgets didn't capture the event, handle arrow events
        if let cosmic::iced_core::Event::Mouse(mouse_event) = &event {
            if let Some(pos) = cursor.position() {
                // Handle arrow drawing mode - press to start, release to end
                if self.arrow_mode {
                    // Check if position is inside selection rectangle
                    let inside_selection = if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        pos.x >= sel_x && pos.x <= sel_x + sel_w &&
                        pos.y >= sel_y && pos.y <= sel_y + sel_h
                    } else {
                        false
                    };
                    
                    match mouse_event {
                        MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                            // Start a new arrow on press
                            let global_x = pos.x + self.output_rect.left as f32;
                            let global_y = pos.y + self.output_rect.top as f32;
                            if let Some(ref on_arrow_start) = self.on_arrow_start {
                                shell.publish(on_arrow_start(global_x, global_y));
                            }
                            return cosmic::iced_core::event::Status::Captured;
                        }
                        MouseEvent::ButtonReleased(Button::Left) if self.arrow_drawing.is_some() => {
                            // Finish the arrow on release
                            let global_x = pos.x + self.output_rect.left as f32;
                            let global_y = pos.y + self.output_rect.top as f32;
                            if let Some(ref on_arrow_end) = self.on_arrow_end {
                                shell.publish(on_arrow_end(global_x, global_y));
                            }
                            return cosmic::iced_core::event::Status::Captured;
                        }
                        _ => {}
                    }
                }
            }
        }
        
        status
    }

    fn mouse_interaction(
        &self,
        state: &Tree,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
        renderer: &cosmic::Renderer,
    ) -> cosmic::iced_core::mouse::Interaction {
        let children = [&self.bg_element, &self.fg_element, &self.menu_element];
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
        cosmic::iced_core::mouse::Interaction::default()
    }

    fn operate(
        &self,
        tree: &mut cosmic::iced_core::widget::Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn cosmic::widget::Operation<()>,
    ) {
        let layout = layout.children().collect::<Vec<_>>();
        let children = [&self.bg_element, &self.fg_element, &self.menu_element];
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

    fn id(&self) -> Option<cosmic::widget::Id> {
        Some(self.id.clone())
    }

    fn set_id(&mut self, _id: cosmic::widget::Id) {
        self.id = _id;
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &self,
        tree: &mut cosmic::iced_core::widget::Tree,
        renderer: &cosmic::Renderer,
        limits: &cosmic::iced_core::layout::Limits,
    ) -> cosmic::iced_core::layout::Node {
        let children = &mut tree.children;
        let bg_image = &mut children[0];
        let bg_node = self
            .bg_element
            .as_widget()
            .layout(bg_image, renderer, limits);
        let fg_node = self
            .fg_element
            .as_widget()
            .layout(&mut children[1], renderer, limits);
        let mut menu_node =
            self.menu_element
                .as_widget()
                .layout(&mut children[2], renderer, limits);
        let menu_bounds = menu_node.bounds();
        let margin = 32.0_f32;
        
        // Position menu based on toolbar_position
        let menu_pos = match self.toolbar_position {
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

        layout::Node::with_children(
            limits.resolve(Length::Fill, Length::Fill, Size::ZERO),
            vec![bg_node, fg_node, menu_node],
        )
    }

    fn draw(
        &self,
        tree: &cosmic::iced_core::widget::Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced_core::renderer::Style,
        layout: cosmic::iced_core::Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
    ) {
        use cosmic::iced_core::Renderer;
        use cosmic::iced_core::text::{Renderer as TextRenderer, Text};
        
        let children = &[&self.bg_element, &self.fg_element, &self.menu_element];
        let mut children_iter = layout.children().zip(children).enumerate();
        
        // Draw bg_element first (screenshot background)
        {
            let (i, (layout, child)) = children_iter.next().unwrap();
            let bg_tree = &tree.children[i];
            child
                .as_widget()
                .draw(bg_tree, renderer, theme, style, layout, cursor, viewport);
        }

        // Draw fg_element (rectangle selection overlay)
        if let Some((i, (layout, child))) = children_iter.next() {
            renderer.with_layer(layout.bounds(), |renderer| {
                let tree = &tree.children[i];
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        }

        // Helper function to build arrow mesh vertices and indices
        fn build_arrow_mesh(
            start_x: f32,
            start_y: f32,
            end_x: f32,
            end_y: f32,
            color: cosmic::iced::Color,
            thickness: f32,
            head_size: f32,
        ) -> Option<(Vec<SolidVertex2D>, Vec<u32>)> {
            let dx = end_x - start_x;
            let dy = end_y - start_y;
            let length = (dx * dx + dy * dy).sqrt();
            if length < 5.0 {
                return None;
            }
            
            // Normalize direction
            let nx = dx / length;
            let ny = dy / length;
            
            // Perpendicular vector for thickness
            let px = -ny * thickness / 2.0;
            let py = nx * thickness / 2.0;
            
            // Shaft end (before arrowhead)
            let shaft_end_x = end_x - nx * head_size;
            let shaft_end_y = end_y - ny * head_size;
            
            // Pack color
            let packed_color = pack(color);
            
            // Vertices for the shaft (4 corners of rotated rectangle)
            // and arrowhead (3 points of triangle)
            let mut vertices = Vec::with_capacity(7);
            
            // Shaft vertices (0-3)
            vertices.push(SolidVertex2D {
                position: [start_x + px, start_y + py],
                color: packed_color,
            });
            vertices.push(SolidVertex2D {
                position: [start_x - px, start_y - py],
                color: packed_color,
            });
            vertices.push(SolidVertex2D {
                position: [shaft_end_x - px, shaft_end_y - py],
                color: packed_color,
            });
            vertices.push(SolidVertex2D {
                position: [shaft_end_x + px, shaft_end_y + py],
                color: packed_color,
            });
            
            // Arrowhead vertices (4-6)
            // Base of arrowhead (wider than shaft)
            let head_width = head_size * 0.5;
            let hpx = -ny * head_width;
            let hpy = nx * head_width;
            
            vertices.push(SolidVertex2D {
                position: [shaft_end_x + hpx, shaft_end_y + hpy],
                color: packed_color,
            });
            vertices.push(SolidVertex2D {
                position: [shaft_end_x - hpx, shaft_end_y - hpy],
                color: packed_color,
            });
            vertices.push(SolidVertex2D {
                position: [end_x, end_y], // Tip of arrow
                color: packed_color,
            });
            
            // Indices: 2 triangles for shaft, 1 triangle for head
            let indices = vec![
                0, 1, 2, // First triangle of shaft
                0, 2, 3, // Second triangle of shaft
                4, 5, 6, // Arrowhead triangle
            ];
            
            Some((vertices, indices))
        }
        
        // Draw arrows on top of the selection using meshes
        let arrow_color = cosmic::iced::Color::from_rgb(0.9, 0.1, 0.1); // Red
        let arrow_thickness = 4.0_f32;
        let head_size = 16.0_f32;
        
        for arrow in &self.arrows {
            // Convert global coordinates to widget-local
            let start_x = arrow.start_x - self.output_rect.left as f32;
            let start_y = arrow.start_y - self.output_rect.top as f32;
            let end_x = arrow.end_x - self.output_rect.left as f32;
            let end_y = arrow.end_y - self.output_rect.top as f32;
            
            if let Some((vertices, indices)) = build_arrow_mesh(
                start_x, start_y, end_x, end_y,
                arrow_color, arrow_thickness, head_size,
            ) {
                renderer.with_layer(*viewport, |renderer| {
                    renderer.draw_mesh(Mesh::Solid {
                        buffers: Indexed { vertices, indices },
                        transformation: cosmic::iced_core::Transformation::IDENTITY,
                        clip_bounds: *viewport,
                    });
                });
            }
        }
        
        // Draw arrow currently being drawn (preview)
        if let Some((start_x, start_y)) = self.arrow_drawing {
            if let Some(cursor_pos) = cursor.position() {
                let local_start_x = start_x - self.output_rect.left as f32;
                let local_start_y = start_y - self.output_rect.top as f32;
                let end_x = cursor_pos.x;
                let end_y = cursor_pos.y;
                
                let preview_color = cosmic::iced::Color::from_rgba(0.9, 0.1, 0.1, 0.7);
                
                if let Some((vertices, indices)) = build_arrow_mesh(
                    local_start_x, local_start_y, end_x, end_y,
                    preview_color, arrow_thickness, head_size,
                ) {
                    renderer.with_layer(*viewport, |renderer| {
                        renderer.draw_mesh(Mesh::Solid {
                            buffers: Indexed { vertices, indices },
                            transformation: cosmic::iced_core::Transformation::IDENTITY,
                            clip_bounds: *viewport,
                        });
                    });
                }
            }
        }

        let cosmic_theme = theme.cosmic();
        let accent_color: cosmic::iced::Color = cosmic_theme.accent_color().into();

        // Draw QR scanning status or QR overlays (toggled off while dragging)
        if self.show_qr_overlays {
            // Show scanning indicator in top-left corner
            if self.qr_scanning {
                let scanning_text = "Scanning for QR codes...";
                let font_size = 16.0_f32;
                let char_width = font_size * 0.55;
                let text_width = scanning_text.len() as f32 * char_width;
                let text_height = font_size * 1.4;
                let padding_h = 16.0;
                let padding_v = 10.0;
                
                let bg_width = text_width + padding_h * 2.0;
                let bg_height = text_height + padding_v * 2.0;
                
                let bg_rect = cosmic::iced_core::Rectangle {
                    x: 20.0,
                    y: 20.0,
                    width: bg_width,
                    height: bg_height,
                };
                
                renderer.with_layer(*viewport, |renderer| {
                    renderer.fill_quad(
                        cosmic::iced_core::renderer::Quad {
                            bounds: bg_rect,
                            border: Border {
                                radius: cosmic_theme.corner_radii.radius_s.into(),
                                width: 2.0,
                                color: accent_color,
                            },
                            shadow: cosmic::iced_core::Shadow::default(),
                        },
                        Background::Color(cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.80)),
                    );
                    
                    let text = Text {
                        content: scanning_text.to_string(),
                        bounds: Size::new(bg_width, bg_height),
                        size: cosmic::iced::Pixels(font_size),
                        line_height: cosmic::iced_core::text::LineHeight::default(),
                        font: cosmic::iced::Font::default(),
                        horizontal_alignment: alignment::Horizontal::Center,
                        vertical_alignment: alignment::Vertical::Center,
                        shaping: cosmic::iced_core::text::Shaping::Advanced,
                        wrapping: cosmic::iced_core::text::Wrapping::None,
                    };
                    
                    renderer.fill_text(
                        text,
                        Point::new(bg_rect.x + bg_width / 2.0, bg_rect.y + bg_height / 2.0),
                        cosmic::iced::Color::WHITE,
                        *viewport,
                    );
                });
            }
            
            // Draw detected QR codes - constrained to selection rectangle
            if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                for (x, y, content) in &self.qr_codes {
                    let font_size = 14.0_f32;
                    let padding = 8.0;
                    
                    // Calculate max label width based on selection rectangle
                    let max_label_width = (sel_w - padding * 4.0).max(80.0).min(400.0);
                    
                    // Estimate number of lines for wrapped text
                    let chars_per_line = (max_label_width / (font_size * 0.55)).max(10.0) as usize;
                    let num_lines = ((content.len() / chars_per_line).max(1) + 1).min(6); // Cap at 6 lines
                    let text_height = (num_lines as f32 * font_size * 1.3).min(sel_h * 0.6);
                    
                    let bg_width = max_label_width + padding * 2.0;
                    let bg_height = text_height + padding * 2.0;
                    
                    // Position centered on QR location, but clamp to selection bounds
                    let mut label_x = *x - bg_width / 2.0;
                    let mut label_y = *y - bg_height / 2.0;
                    
                    // Clamp to selection rectangle
                    label_x = label_x.max(sel_x + padding).min(sel_x + sel_w - bg_width - padding);
                    label_y = label_y.max(sel_y + padding).min(sel_y + sel_h - bg_height - padding);
                    
                    let bg_rect = cosmic::iced_core::Rectangle {
                        x: label_x,
                        y: label_y,
                        width: bg_width,
                        height: bg_height,
                    };
                    
                    // Draw in a layer to ensure proper rendering
                    renderer.with_layer(*viewport, |renderer| {
                        // Draw background with 80% opacity
                        renderer.fill_quad(
                            cosmic::iced_core::renderer::Quad {
                                bounds: bg_rect,
                                border: Border {
                                    radius: cosmic_theme.corner_radii.radius_s.into(),
                                    width: 2.0,
                                    color: accent_color,
                                },
                                shadow: cosmic::iced_core::Shadow::default(),
                            },
                            Background::Color(cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.80)),
                        );
                        
                        // Draw text with word wrapping
                        let text = Text {
                            content: content.clone(),
                            bounds: Size::new(max_label_width, text_height),
                            size: cosmic::iced::Pixels(font_size),
                            line_height: cosmic::iced_core::text::LineHeight::Relative(1.3),
                            font: cosmic::iced::Font::default(),
                            horizontal_alignment: alignment::Horizontal::Left,
                            vertical_alignment: alignment::Vertical::Top,
                            shaping: cosmic::iced_core::text::Shaping::Advanced,
                            wrapping: cosmic::iced_core::text::Wrapping::Word,
                        };
                        
                        renderer.fill_text(
                            text,
                            Point::new(bg_rect.x + padding, bg_rect.y + padding),
                            cosmic::iced::Color::WHITE,
                            *viewport,
                        );
                    });
                }
            }
        }

        // Show OCR status indicator (only when downloading, running, or error - not when done or idle)
        let show_ocr_status = matches!(&self.ocr_status, OcrStatus::DownloadingModels | OcrStatus::Running | OcrStatus::Error(_));
        if show_ocr_status {
            let status_text = match &self.ocr_status {
                OcrStatus::DownloadingModels => "Downloading OCR models...".to_string(),
                OcrStatus::Running => "Running OCR...".to_string(),
                OcrStatus::Error(err) => format!("OCR error: {}", if err.len() > 40 { format!("{}...", &err[..37]) } else { err.clone() }),
                _ => unreachable!(),
            };
            
            let font_size = 16.0_f32;
            let char_width = font_size * 0.55;
            let text_width = status_text.len() as f32 * char_width;
            let text_height = font_size * 1.4;
            let padding_h = 16.0;
            let padding_v = 10.0;
            
            let bg_width = text_width + padding_h * 2.0;
            let bg_height = text_height + padding_v * 2.0;
            
            // Position below QR scanning indicator if it's showing
            let y_offset = if self.qr_scanning { 60.0 } else { 20.0 };
            
            let bg_rect = cosmic::iced_core::Rectangle {
                x: 20.0,
                y: y_offset,
                width: bg_width,
                height: bg_height,
            };
            
            // Choose border color based on status
            let border_color = match &self.ocr_status {
                OcrStatus::Error(_) => cosmic::iced::Color::from_rgb(0.9, 0.2, 0.2), // Red
                _ => accent_color, // Accent for in-progress
            };
            
            renderer.with_layer(*viewport, |renderer| {
                renderer.fill_quad(
                    cosmic::iced_core::renderer::Quad {
                        bounds: bg_rect,
                        border: Border {
                            radius: cosmic_theme.corner_radii.radius_s.into(),
                            width: 2.0,
                            color: border_color,
                        },
                        shadow: cosmic::iced_core::Shadow::default(),
                    },
                    Background::Color(cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.85)),
                );
                
                let text = Text {
                    content: status_text,
                    bounds: Size::new(bg_width, bg_height),
                    size: cosmic::iced::Pixels(font_size),
                    line_height: cosmic::iced_core::text::LineHeight::default(),
                    font: cosmic::iced::Font::default(),
                    horizontal_alignment: alignment::Horizontal::Center,
                    vertical_alignment: alignment::Vertical::Center,
                    shaping: cosmic::iced_core::text::Shaping::Advanced,
                    wrapping: cosmic::iced_core::text::Wrapping::None,
                };
                
                renderer.fill_text(
                    text,
                    Point::new(bg_rect.x + bg_width / 2.0, bg_rect.y + bg_height / 2.0),
                    cosmic::iced::Color::WHITE,
                    *viewport,
                );
            });
        }

        // Draw OCR overlays as colored rectangles - only when not dragging
        if self.show_qr_overlays {
            // Color palette for different block numbers
            let block_colors = [
                cosmic::iced::Color::from_rgb(0.2, 0.6, 0.9),  // Blue
                cosmic::iced::Color::from_rgb(0.9, 0.3, 0.3),  // Red
                cosmic::iced::Color::from_rgb(0.3, 0.8, 0.3),  // Green
                cosmic::iced::Color::from_rgb(0.9, 0.6, 0.2),  // Orange
                cosmic::iced::Color::from_rgb(0.7, 0.3, 0.9),  // Purple
                cosmic::iced::Color::from_rgb(0.2, 0.8, 0.8),  // Cyan
                cosmic::iced::Color::from_rgb(0.9, 0.9, 0.2),  // Yellow
                cosmic::iced::Color::from_rgb(0.9, 0.4, 0.7),  // Pink
            ];
            
            for (left, top, width, height, block_num) in &self.ocr_overlays {
                let color_idx = (*block_num as usize) % block_colors.len();
                let border_color = block_colors[color_idx];
                
                let rect = cosmic::iced_core::Rectangle {
                    x: *left,
                    y: *top,
                    width: *width,
                    height: *height,
                };
                
                renderer.with_layer(*viewport, |renderer| {
                    renderer.fill_quad(
                        cosmic::iced_core::renderer::Quad {
                            bounds: rect,
                            border: Border {
                                radius: 2.0.into(),
                                width: 2.0,
                                color: border_color,
                            },
                            shadow: cosmic::iced_core::Shadow::default(),
                        },
                        Background::Color(cosmic::iced::Color::TRANSPARENT),
                    );
                });
            }
        }

        // Menu bar hidden - using radial menu instead (right-click)
        // Consume the iterator to avoid unused variable warning
        let _ = children_iter;
        
        // Draw menu_element (bottom toolbar)
        if let Some((i, (layout, child))) = children_iter.next() {
            renderer.with_layer(layout.bounds(), |renderer| {
                let tree = &tree.children[i];
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        }
    }

    fn drag_destinations(
        &self,
        state: &cosmic::iced_core::widget::Tree,
        layout: cosmic::iced_core::Layout<'_>,
        renderer: &cosmic::Renderer,
        dnd_rectangles: &mut cosmic::iced_core::clipboard::DndDestinationRectangles,
    ) {
        let children = &[&self.bg_element, &self.fg_element, &self.menu_element];
        for (i, (layout, child)) in layout.children().zip(children).enumerate() {
            let state = &state.children[i];
            child
                .as_widget()
                .drag_destinations(state, layout, renderer, dnd_rectangles);
        }
    }
}

impl<'a, Message> From<ScreenshotSelection<'a, Message>> for cosmic::Element<'a, Message>
where
    Message: 'static + Clone,
{
    fn from(w: ScreenshotSelection<'a, Message>) -> cosmic::Element<'a, Message> {
        Element::new(w)
    }
}

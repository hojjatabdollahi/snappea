use std::{borrow::Cow, collections::HashMap};

use cosmic::{
    Element,
    cosmic_theme::Spacing,
    iced::{self, window},
    iced_core::{
        Background, Border, ContentFit, Degrees, Layout, Length, Point, Size, alignment,
        gradient::Linear, layout, overlay, widget::Tree,
    },
    iced_widget::canvas,
    iced_widget::graphics::{
        Mesh,
        color::{pack, Packed},
        mesh::{Indexed, Renderer as MeshRenderer, SolidVertex2D},
    },
    widget::{Row, button, horizontal_space, image, layer_container},
};
use cosmic_bg_config::Source;
use wayland_client::protocol::wl_output::WlOutput;

use crate::{
    app::OutputState,
    config::ShapeTool,
    screenshot::{
        Annotation, ArrowAnnotation, Choice, CircleOutlineAnnotation, DetectedQrCode, OcrStatus,
        OcrTextOverlay, PixelateAnnotation, Rect, RectOutlineAnnotation, RedactAnnotation,
        ScreenshotImage, ToolbarPosition,
    },
};

use super::{
    output_selection::OutputSelection,
    rectangle_selection::{DragState, RectangleSelection},
    settings_drawer::build_settings_drawer,
    tool_button::{build_redact_popup, build_shape_popup},
};

use super::toolbar::build_toolbar;

struct ShapesOverlay<'a, Message: Clone + 'static> {
    // Selection rect in output-local coordinates (x, y, w, h)
    selection_rect: Option<(f32, f32, f32, f32)>,
    // Output rect for global offset
    output_rect: Rect,
    // Existing shapes in global coordinates
    circles: Vec<CircleOutlineAnnotation>,
    rect_outlines: Vec<RectOutlineAnnotation>,
    // Drawing modes
    circle_mode: bool,
    rect_outline_mode: bool,
    // Current drawing start in global coordinates (if any)
    circle_drawing: Option<(f32, f32)>,
    rect_outline_drawing: Option<(f32, f32)>,
    // Messages
    on_circle_start: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    on_circle_end: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    on_rect_start: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    on_rect_end: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    // Shape styling
    shape_color: crate::config::ShapeColor,
    shape_shadow: bool,
}

#[derive(Debug, Default)]
struct ShapesState {
    ctrl_down: bool,
    ctrl_latched: bool,
}

impl ShapesState {
    fn latch_ctrl_if_needed(&mut self, drawing_active: bool) {
        if drawing_active && self.ctrl_down {
            self.ctrl_latched = true;
        }
    }
}

impl<'a, Message: Clone + 'static> ShapesOverlay<'a, Message> {
    fn constrain_end(sx: f32, sy: f32, ex: f32, ey: f32) -> (f32, f32) {
        let dx = ex - sx;
        let dy = ey - sy;
        let side = dx.abs().min(dy.abs());
        let sign_x = if dx < 0.0 { -1.0 } else { 1.0 };
        let sign_y = if dy < 0.0 { -1.0 } else { 1.0 };
        (sx + side * sign_x, sy + side * sign_y)
    }
}

impl<'a, Message: Clone + 'static> canvas::Program<Message, cosmic::Theme, cosmic::Renderer>
    for ShapesOverlay<'a, Message>
{
    type State = ShapesState;

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: cosmic::iced_core::Rectangle,
        cursor: cosmic::iced_core::mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        use cosmic::iced_core::keyboard;
        use cosmic::iced_core::mouse::{Button, Event as MouseEvent};

        // Margin for shape clamping (0 = clamp to exact edge)
        const ANNOTATION_MARGIN: f32 = 0.0;
        
        // Helper to clamp and check inner bounds
        let (inner_x, inner_y, inner_w, inner_h) = if let Some((x, y, w, h)) = self.selection_rect {
            (x + ANNOTATION_MARGIN, y + ANNOTATION_MARGIN, 
             w - 2.0 * ANNOTATION_MARGIN, h - 2.0 * ANNOTATION_MARGIN)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        
        let clamp_pos = |px: f32, py: f32| -> (f32, f32) {
            if let Some((x, y, w, h)) = self.selection_rect {
                let min_x = x + ANNOTATION_MARGIN;
                let max_x = x + w - ANNOTATION_MARGIN;
                let min_y = y + ANNOTATION_MARGIN;
                let max_y = y + h - ANNOTATION_MARGIN;
                (px.clamp(min_x, max_x), py.clamp(min_y, max_y))
            } else {
                (px, py)
            }
        };

        match event {
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) => {
                state.ctrl_down = mods.control();
                state.latch_ctrl_if_needed(
                    self.circle_drawing.is_some() || self.rect_outline_drawing.is_some(),
                );
                return (canvas::event::Status::Captured, None);
            }
            canvas::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
                let Some(pos) = cursor.position_in(bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                // Check if inside inner bounds (with margin)
                let inside = inner_w > 0.0 && inner_h > 0.0
                    && pos.x >= inner_x && pos.x <= inner_x + inner_w 
                    && pos.y >= inner_y && pos.y <= inner_y + inner_h;
                if !inside {
                    return (canvas::event::Status::Ignored, None);
                }

                // Clamp and convert to global coordinates
                let (cx, cy) = clamp_pos(pos.x, pos.y);
                let gx = cx + self.output_rect.left as f32;
                let gy = cy + self.output_rect.top as f32;

                if self.circle_mode {
                    state.ctrl_latched = state.ctrl_down;
                    if let Some(ref cb) = self.on_circle_start {
                        return (canvas::event::Status::Captured, Some(cb(gx, gy)));
                    }
                }
                if self.rect_outline_mode {
                    state.ctrl_latched = state.ctrl_down;
                    if let Some(ref cb) = self.on_rect_start {
                        return (canvas::event::Status::Captured, Some(cb(gx, gy)));
                    }
                }
            }
            canvas::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                let Some(pos) = cursor.position_in(bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                // Clamp and convert to global coordinates
                let (cx, cy) = clamp_pos(pos.x, pos.y);
                let gx = cx + self.output_rect.left as f32;
                let gy = cy + self.output_rect.top as f32;

                if self.circle_mode && self.circle_drawing.is_some() {
                    let (sx, sy) = self.circle_drawing.unwrap_or((gx, gy));
                    let (ex, ey) = if state.ctrl_latched || state.ctrl_down {
                        Self::constrain_end(sx, sy, gx, gy)
                    } else {
                        (gx, gy)
                    };
                    state.ctrl_latched = false;
                    if let Some(ref cb) = self.on_circle_end {
                        return (canvas::event::Status::Captured, Some(cb(ex, ey)));
                    }
                }

                if self.rect_outline_mode && self.rect_outline_drawing.is_some() {
                    let (sx, sy) = self.rect_outline_drawing.unwrap_or((gx, gy));
                    let (ex, ey) = if state.ctrl_latched || state.ctrl_down {
                        Self::constrain_end(sx, sy, gx, gy)
                    } else {
                        (gx, gy)
                    };
                    state.ctrl_latched = false;
                    if let Some(ref cb) = self.on_rect_end {
                        return (canvas::event::Status::Captured, Some(cb(ex, ey)));
                    }
                }
            }
            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: cosmic::iced_core::Rectangle,
        cursor: cosmic::iced_core::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        use canvas::{Frame, Path, Stroke};
        use cosmic::iced_core::{Color, Point, Size};

        let mut frame = Frame::new(renderer, bounds.size());

        let shadow_stroke = Stroke {
            style: Color::from_rgba(0.0, 0.0, 0.0, 0.9).into(),
            width: 5.0,
            ..Stroke::default()
        };

        // Draw rectangle outlines with per-annotation colors
        for r in &self.rect_outlines {
            let rect_color: Color = r.color.into();
            let stroke = Stroke {
                style: rect_color.into(),
                width: 3.0,
                ..Stroke::default()
            };
            let x1 = r.start_x - self.output_rect.left as f32;
            let y1 = r.start_y - self.output_rect.top as f32;
            let x2 = r.end_x - self.output_rect.left as f32;
            let y2 = r.end_y - self.output_rect.top as f32;
            let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
            let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
            let path = Path::rectangle(
                Point::new(min_x, min_y),
                Size::new((max_x - min_x).max(1.0), (max_y - min_y).max(1.0)),
            );
            if r.shadow {
                frame.stroke(&path, shadow_stroke);
            }
            frame.stroke(&path, stroke);
        }

        // Draw circle/ellipse outlines with per-annotation colors
        for c in &self.circles {
            let circle_color: Color = c.color.into();
            let stroke = Stroke {
                style: circle_color.into(),
                width: 3.0,
                ..Stroke::default()
            };
            let x1 = c.start_x - self.output_rect.left as f32;
            let y1 = c.start_y - self.output_rect.top as f32;
            let x2 = c.end_x - self.output_rect.left as f32;
            let y2 = c.end_y - self.output_rect.top as f32;
            let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
            let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
            let cx = (min_x + max_x) * 0.5;
            let cy = (min_y + max_y) * 0.5;
            let rx = ((max_x - min_x) * 0.5).max(1.0);
            let ry = ((max_y - min_y) * 0.5).max(1.0);
            let approx_r = rx.max(ry);
            let segs = ((approx_r * 0.35).clamp(32.0, 128.0)) as usize;
            let step = std::f32::consts::TAU / segs as f32;

            let path = Path::new(|b| {
                b.move_to(Point::new(cx + rx, cy));
                for i in 1..=segs {
                    let t = i as f32 * step;
                    b.line_to(Point::new(cx + rx * t.cos(), cy + ry * t.sin()));
                }
            });
            if c.shadow {
                frame.stroke(&path, shadow_stroke);
            }
            frame.stroke(&path, stroke);
        }

        // Live previews (during drag)
        let constrain = state.ctrl_down || state.ctrl_latched;
        let shape_color: Color = self.shape_color.into();
        let preview_color = Color {
            a: 0.7,
            ..shape_color
        };
        let preview_stroke = Stroke {
            style: preview_color.into(),
            width: 3.0,
            ..Stroke::default()
        };
        let preview_shadow_stroke = Stroke {
            style: Color::from_rgba(0.0, 0.0, 0.0, 0.6).into(),
            width: 5.0,
            ..Stroke::default()
        };

        if let Some((sx_g, sy_g)) = self.rect_outline_drawing {
            if let Some(pos) = cursor.position_in(bounds) {
                let sx = sx_g - self.output_rect.left as f32;
                let sy = sy_g - self.output_rect.top as f32;
                let mut ex = pos.x;
                let mut ey = pos.y;
                if constrain {
                    (ex, ey) = Self::constrain_end(sx, sy, ex, ey);
                }
                let (min_x, max_x) = if sx < ex { (sx, ex) } else { (ex, sx) };
                let (min_y, max_y) = if sy < ey { (sy, ey) } else { (ey, sy) };
                let path = Path::rectangle(
                    Point::new(min_x, min_y),
                    Size::new((max_x - min_x).max(1.0), (max_y - min_y).max(1.0)),
                );
                if self.shape_shadow {
                    frame.stroke(&path, preview_shadow_stroke);
                }
                frame.stroke(&path, preview_stroke);
            }
        }

        if let Some((sx_g, sy_g)) = self.circle_drawing {
            if let Some(pos) = cursor.position_in(bounds) {
                let sx = sx_g - self.output_rect.left as f32;
                let sy = sy_g - self.output_rect.top as f32;
                let mut ex = pos.x;
                let mut ey = pos.y;
                if constrain {
                    (ex, ey) = Self::constrain_end(sx, sy, ex, ey);
                }
                let (min_x, max_x) = if sx < ex { (sx, ex) } else { (ex, sx) };
                let (min_y, max_y) = if sy < ey { (sy, ey) } else { (ey, sy) };
                let cx = (min_x + max_x) * 0.5;
                let cy = (min_y + max_y) * 0.5;
                let rx = ((max_x - min_x) * 0.5).max(1.0);
                let ry = ((max_y - min_y) * 0.5).max(1.0);
                let approx_r = rx.max(ry);
                let segs = ((approx_r * 0.35).clamp(32.0, 128.0)) as usize;
                let step = std::f32::consts::TAU / segs as f32;
                let path = Path::new(|b| {
                    b.move_to(Point::new(cx + rx, cy));
                    for i in 1..=segs {
                        let t = i as f32 * step;
                        b.line_to(Point::new(cx + rx * t.cos(), cy + ry * t.sin()));
                    }
                });
                if self.shape_shadow {
                    frame.stroke(&path, preview_shadow_stroke);
                }
                frame.stroke(&path, preview_stroke);
            }
        }

        vec![frame.into_geometry()]
    }
}

/// Check if a string looks like a URL
fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www.")
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
        screen_size: (u32, u32),
    ) -> Self {
        let (image_handle, image_size) = if let Some(window_index) = window_index {
            toplevel_images
                .get(&output_name)
                .and_then(|imgs| imgs.get(window_index))
                .map(|img| {
                    let orig_width = img.rgba.width();
                    let orig_height = img.rgba.height();

                    // Use 85% of screen size as the maximum preview size
                    let max_width = (screen_size.0 as f32 * 0.85) as u32;
                    let max_height = (screen_size.1 as f32 * 0.85) as u32;

                    // If the image is larger than the screen, create a scaled thumbnail
                    // using high-quality Lanczos3 filtering for better downscaling
                    if orig_width > max_width || orig_height > max_height {
                        let scale = (max_width as f32 / orig_width as f32)
                            .min(max_height as f32 / orig_height as f32);
                        let new_width = (orig_width as f32 * scale) as u32;
                        let new_height = (orig_height as f32 * scale) as u32;

                        let scaled = ::image::imageops::resize(
                            &img.rgba,
                            new_width,
                            new_height,
                            ::image::imageops::FilterType::Lanczos3,
                        );

                        let handle = cosmic::widget::image::Handle::from_rgba(
                            new_width,
                            new_height,
                            scaled.into_vec(),
                        );
                        (Some(handle), (new_width, new_height))
                    } else {
                        // Image fits on screen - use original at 1:1
                        (Some(img.handle.clone()), (orig_width, orig_height))
                    }
                })
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
    pub fn image_bounds(
        &self,
        layout_bounds: cosmic::iced_core::Rectangle,
    ) -> cosmic::iced_core::Rectangle {
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

impl<Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for SelectedImageWidget
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &self,
        _tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        limits: &cosmic::iced::Limits,
    ) -> layout::Node {
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

        // Draw dark overlay outside the selected window (60% opacity, matching region selection)
        let overlay_color = Background::Color(cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6));

        // Top strip
        if image_bounds.y > layout_bounds.y {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: layout_bounds.x,
                        y: layout_bounds.y,
                        width: layout_bounds.width,
                        height: image_bounds.y - layout_bounds.y,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                overlay_color,
            );
        }

        // Bottom strip
        let image_bottom = image_bounds.y + image_bounds.height;
        let layout_bottom = layout_bounds.y + layout_bounds.height;
        if image_bottom < layout_bottom {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: layout_bounds.x,
                        y: image_bottom,
                        width: layout_bounds.width,
                        height: layout_bottom - image_bottom,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                overlay_color,
            );
        }

        // Left strip (between top and bottom)
        if image_bounds.x > layout_bounds.x {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: layout_bounds.x,
                        y: image_bounds.y,
                        width: image_bounds.x - layout_bounds.x,
                        height: image_bounds.height,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                overlay_color,
            );
        }

        // Right strip (between top and bottom)
        let image_right = image_bounds.x + image_bounds.width;
        let layout_right = layout_bounds.x + layout_bounds.width;
        if image_right < layout_right {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: cosmic::iced_core::Rectangle {
                        x: image_right,
                        y: image_bounds.y,
                        width: layout_right - image_right,
                        height: image_bounds.height,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                overlay_color,
            );
        }

        // Draw the image with linear filtering for better quality when scaling down
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
            (
                image_bounds.x + image_bounds.width,
                image_bounds.y + image_bounds.height,
            ),
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
    /// Circle/ellipse outline annotations (no fill)
    pub circles: Vec<CircleOutlineAnnotation>,
    /// Whether circle/ellipse mode is active
    pub circle_mode: bool,
    /// Circle/ellipse currently being drawn (start point)
    pub circle_drawing: Option<(f32, f32)>,
    /// Callbacks for circle mode
    pub on_circle_toggle: Option<Msg>,
    /// Rectangle outline annotations (no fill)
    pub rect_outlines: Vec<RectOutlineAnnotation>,
    /// Whether rectangle outline mode is active
    pub rect_outline_mode: bool,
    /// Rectangle outline currently being drawn (start point)
    pub rect_outline_drawing: Option<(f32, f32)>,
    /// Callbacks for rectangle outline mode
    pub on_rect_outline_toggle: Option<Msg>,
    /// Redaction annotations
    pub redactions: Vec<RedactAnnotation>,
    /// Whether redact mode is active
    pub redact_mode: bool,
    /// Redaction currently being drawn (start point)
    pub redact_drawing: Option<(f32, f32)>,
    /// Callbacks for redact mode
    pub on_redact_toggle: Option<Msg>,
    pub on_redact_start: Option<Box<dyn Fn(f32, f32) -> Msg + 'a>>,
    pub on_redact_end: Option<Box<dyn Fn(f32, f32) -> Msg + 'a>>,
    /// Pixelation annotations
    pub pixelations: Vec<PixelateAnnotation>,
    /// Unified annotations array (for proper draw order)
    pub annotations: &'a [Annotation],
    /// Whether pixelate mode is active
    pub pixelate_mode: bool,
    /// Pixelation currently being drawn (start point)
    pub pixelate_drawing: Option<(f32, f32)>,
    /// Callbacks for pixelate mode
    pub on_pixelate_toggle: Option<Msg>,
    pub on_pixelate_start: Option<Box<dyn Fn(f32, f32) -> Msg + 'a>>,
    pub on_pixelate_end: Option<Box<dyn Fn(f32, f32) -> Msg + 'a>>,
    /// Callback for clearing all annotations
    pub on_clear_annotations: Option<Msg>,
    /// Toolbar position
    pub toolbar_position: ToolbarPosition,
    /// Callback for toolbar position change
    pub on_toolbar_position: Option<Box<dyn Fn(ToolbarPosition) -> Msg + 'a>>,
    /// Callback for opening URLs from QR codes
    pub on_open_url: Option<Box<dyn Fn(String) -> Msg + 'a>>,
    /// Whether settings drawer is open
    pub settings_drawer_open: bool,
    /// Whether magnifier is enabled
    pub magnifier_enabled: bool,
    /// Callback for toggling settings drawer
    pub on_settings_toggle: Option<Msg>,
    /// Callback for toggling magnifier
    pub on_magnifier_toggle: Option<Msg>,
    /// Save location setting
    pub save_location: crate::config::SaveLocation,
    /// Callback for setting save location to Pictures
    pub on_save_location_pictures: Option<Msg>,
    /// Callback for setting save location to Documents
    pub on_save_location_documents: Option<Msg>,
    /// Whether to copy to clipboard on save
    pub copy_to_clipboard_on_save: bool,
    /// Callback for toggling copy on save
    pub on_copy_on_save_toggle: Option<Msg>,
    /// Settings drawer element (only present when drawer is open)
    pub settings_drawer_element: Option<Element<'a, Msg>>,
    /// Shape settings popup element (only present when popup is open)
    pub shape_popup_element: Option<Element<'a, Msg>>,
    /// Redact settings popup element (only present when popup is open)
    pub redact_popup_element: Option<Element<'a, Msg>>,
    /// Canvas overlay for circle/rectangle outline rendering and input
    pub shapes_element: Element<'a, Msg>,
    /// Primary shape tool shown in button
    pub primary_shape_tool: ShapeTool,
    /// Whether shape settings popup is open
    pub shape_popup_open: bool,
    /// Current shape color
    pub shape_color: crate::config::ShapeColor,
    /// Whether shape shadow is enabled
    pub shape_shadow: bool,
    /// Callback for toggling shape mode
    pub on_shape_toggle: Option<Msg>,
    /// Callback for toggling shape mode (normal click)
    pub on_shape_popup_toggle: Option<Msg>,
    /// Callback for opening shape popup (right-click or long-press)
    pub on_open_shape_popup: Option<Msg>,
    /// Callback for closing shape popup without deactivating shape mode (for click-outside)
    pub on_close_shape_popup: Option<Msg>,
    /// Callback for setting the primary shape tool
    pub on_set_shape_tool: Option<Box<dyn Fn(ShapeTool) -> Msg + 'a>>,
    /// Reference to the screenshot image for pixelation preview
    pub screenshot_image: &'a ::image::RgbaImage,
    /// Scale factor (physical pixels per logical pixel)
    pub image_scale: f32,
    /// Pixelation block size for preview rendering
    pub pixelation_block_size: u32,
    /// Optional window image for window mode (for correct pixelation preview)
    pub window_image: Option<&'a ::image::RgbaImage>,
    /// Window display info: (display_x, display_y, display_width, display_height, display_to_image_scale)
    pub window_display_info: Option<(f32, f32, f32, f32, f32)>,
    /// Callback for setting shape color
    pub on_set_shape_color: Option<Box<dyn Fn(crate::config::ShapeColor) -> Msg + 'a>>,
    /// Callback for toggling shape shadow
    pub on_toggle_shape_shadow: Option<Msg>,
    /// Primary redact tool shown in button
    pub primary_redact_tool: crate::config::RedactTool,
    /// Whether redact settings popup is open
    pub redact_popup_open: bool,
    /// Callback for toggling redact mode (normal click)
    pub on_redact_popup_toggle: Option<Msg>,
    /// Callback for opening redact popup (right-click or long-press)
    pub on_open_redact_popup: Option<Msg>,
    /// Callback for closing redact popup without deactivating mode
    pub on_close_redact_popup: Option<Msg>,
    /// Callback for setting the primary redact tool
    pub on_set_redact_tool: Option<Box<dyn Fn(crate::config::RedactTool) -> Msg + 'a>>,
    /// Callback for clearing redactions only
    pub on_clear_redactions: Option<Msg>,
    /// Whether there are any redactions (for enable/disable clear button)
    pub has_any_redactions: bool,
    /// Whether there are any annotations (for clear button in popup)
    pub has_any_annotations: bool,
    /// Whether this is the active output (where annotations are allowed)
    /// False when another output has the selection, meaning annotations should be blocked
    pub is_active_output: bool,
    /// Whether there's a confirmed selection (for showing dark overlay on non-active screens)
    pub has_confirmed_selection: bool,
    /// Callback to switch to screen picker mode (when user tries to draw on non-active screen)
    pub on_select_screen: Option<Msg>,
}

impl<'a, Msg> ScreenshotSelection<'a, Msg>
where
    Msg: 'static + Clone,
{
    #[allow(clippy::too_many_arguments)]
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
        toplevel_images: &'a HashMap<String, Vec<ScreenshotImage>>,
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
        circles: &[CircleOutlineAnnotation],
        circle_mode: bool,
        circle_drawing: Option<(f32, f32)>,
        on_circle_toggle: Msg,
        on_circle_start: impl Fn(f32, f32) -> Msg + 'a,
        on_circle_end: impl Fn(f32, f32) -> Msg + 'a,
        rect_outlines: &[RectOutlineAnnotation],
        rect_outline_mode: bool,
        rect_outline_drawing: Option<(f32, f32)>,
        on_rect_outline_toggle: Msg,
        on_rect_outline_start: impl Fn(f32, f32) -> Msg + 'a,
        on_rect_outline_end: impl Fn(f32, f32) -> Msg + 'a,
        redactions: &[RedactAnnotation],
        redact_mode: bool,
        redact_drawing: Option<(f32, f32)>,
        on_redact_toggle: Msg,
        on_redact_start: impl Fn(f32, f32) -> Msg + 'a,
        on_redact_end: impl Fn(f32, f32) -> Msg + 'a,
        pixelations: &[PixelateAnnotation],
        annotations: &'a [Annotation],
        pixelate_mode: bool,
        pixelate_drawing: Option<(f32, f32)>,
        on_pixelate_toggle: Msg,
        on_pixelate_start: impl Fn(f32, f32) -> Msg + 'a,
        on_pixelate_end: impl Fn(f32, f32) -> Msg + 'a,
        on_clear_annotations: Msg,
        toolbar_position: ToolbarPosition,
        on_toolbar_position: impl Fn(ToolbarPosition) -> Msg + 'a,
        on_open_url: impl Fn(String) -> Msg + 'a,
        settings_drawer_open: bool,
        magnifier_enabled: bool,
        on_settings_toggle: Msg,
        on_magnifier_toggle: Msg,
        save_location: crate::config::SaveLocation,
        on_save_location_pictures: Msg,
        on_save_location_documents: Msg,
        copy_to_clipboard_on_save: bool,
        on_copy_on_save_toggle: Msg,
        output_count: usize,
        highlighted_window_index: usize,
        focused_output_index: usize,
        current_output_index: usize,
        primary_shape_tool: ShapeTool,
        shape_popup_open: bool,
        shape_color: crate::config::ShapeColor,
        shape_shadow: bool,
        on_shape_toggle: Msg,
        on_shape_popup_toggle: Msg,
        on_open_shape_popup: Msg,
        on_close_shape_popup: Msg,
        on_set_shape_tool: impl Fn(ShapeTool) -> Msg + 'a + Clone,
        on_set_shape_color: impl Fn(crate::config::ShapeColor) -> Msg + 'a,
        on_toggle_shape_shadow: Msg,
        has_any_annotations: bool,
        primary_redact_tool: crate::config::RedactTool,
        redact_popup_open: bool,
        on_redact_popup_toggle: Msg,
        on_open_redact_popup: Msg,
        on_close_redact_popup: Msg,
        on_set_redact_tool: impl Fn(crate::config::RedactTool) -> Msg + 'a + Clone,
        on_clear_redactions: Msg,
        has_any_redactions: bool,
        pixelation_block_size: u32,
        on_set_pixelation_size: impl Fn(u32) -> Msg + 'a,
        on_save_pixelation_size: Msg,
        on_confirm_selection: Msg,
        is_active_output: bool,
        has_confirmed_selection: bool,
        on_select_screen: Msg,
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
                redact_mode,
                pixelate_mode,
                circle_mode,
                rect_outline_mode,
                magnifier_enabled,
            )
            .into(),
            Choice::Output(None) => {
                // Screen picker mode - show hint and highlight on focused output
                let is_focused = current_output_index == focused_output_index;
                OutputSelection::new(on_output_change(output.output.clone()))
                    .picker_mode(true)
                    .focused(is_focused)
                    .on_click(on_confirm_selection.clone())
                    .into()
            }
            Choice::Output(Some(ref selected_output)) => {
                // Confirmed mode - show selection frame only on the confirmed output
                let is_selected = selected_output == &output.name;
                OutputSelection::new(on_output_change(output.output.clone()))
                    .picker_mode(false)
                    .selected(is_selected)
                    .into()
            }
            Choice::Window(_, None) => {
                // Window picker mode - show all windows as buttons for THIS output
                // Each output shows its own windows, not the output from the Choice
                let imgs = toplevel_images
                    .get(&output.name)
                    .map(|x| x.as_slice())
                    .unwrap_or_default();
                let total_img_width = imgs.iter().map(|img| img.width()).sum::<u32>();
                // Only show highlight on the focused output
                let is_focused_output = current_output_index == focused_output_index;

                let img_buttons = imgs.iter().enumerate().map(|(i, img)| {
                    let portion =
                        (img.width() as u64 * u16::MAX as u64 / total_img_width as u64).max(1);
                    let is_highlighted = is_focused_output && i == highlighted_window_index;
                    layer_container(
                        button::custom(
                            image::Image::new(img.handle.clone())
                                .content_fit(ContentFit::ScaleDown),
                        )
                        .on_press(toplevel_chosen(output.name.clone(), i))
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
                // Selected window mode - show the window image with border (only on matching output)
                let screen_size = (output.logical_size.0 as u32, output.logical_size.1 as u32);
                SelectedImageWidget::new(win_output.clone(), Some(win_index), toplevel_images, screen_size)
                    .into()
            }
            Choice::Window(_, Some(_)) => {
                // Window selected on a different output - show nothing (just the background screenshot)
                cosmic::widget::horizontal_space().width(Length::Fill).into()
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
        log::debug!(
            "Widget received {} OCR overlays, filtering for output '{}'",
            ocr_overlays.len(),
            output.name
        );
        let ocr_overlays_for_output: Vec<(f32, f32, f32, f32, i32)> = ocr_overlays
            .iter()
            .filter(|o| {
                let matches = o.output_name == output.name;
                log::debug!("  Overlay output '{}' matches: {}", o.output_name, matches);
                matches
            })
            .map(|o| (o.left, o.top, o.width, o.height, o.block_num))
            .collect();
        log::debug!(
            "After filtering: {} OCR overlays for this output",
            ocr_overlays_for_output.len()
        );

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
                // NOTE: Match the EXACT logic in SelectedImageWidget (pre-scale + display scale)
                if let Some(img) = toplevel_images
                    .get(win_output)
                    .and_then(|imgs| imgs.get(*win_idx))
                {
                    let orig_width = img.rgba.width() as f32;
                    let orig_height = img.rgba.height() as f32;
                    let output_width = output.logical_size.0 as f32;
                    let output_height = output.logical_size.1 as f32;

                    // Step 1: Calculate the pre-scaled thumbnail size (same as SelectedImageWidget)
                    let max_width = output_width * 0.85;
                    let max_height = output_height * 0.85;
                    let (thumb_width, thumb_height) = if orig_width > max_width || orig_height > max_height {
                        let pre_scale = (max_width / orig_width).min(max_height / orig_height);
                        (orig_width * pre_scale, orig_height * pre_scale)
                    } else {
                        (orig_width, orig_height)
                    };

                    // Step 2: Calculate display position (centering the thumbnail with 20px margin)
                    let available_width = output_width - 20.0;
                    let available_height = output_height - 20.0;
                    let scale_x = available_width / thumb_width;
                    let scale_y = available_height / thumb_height;
                    let scale = scale_x.min(scale_y).min(1.0);

                    let display_width = thumb_width * scale;
                    let display_height = thumb_height * scale;
                    let x = (output_width - display_width) / 2.0;
                    let y = (output_height - display_height) / 2.0;

                    Some((x, y, display_width, display_height))
                } else {
                    None
                }
            }
            Choice::Output(Some(_)) => {
                // For confirmed output mode, the entire output is the selection area
                Some((
                    0.0,
                    0.0,
                    output.logical_size.0 as f32,
                    output.logical_size.1 as f32,
                ))
            }
            _ => None,
        };

        // For window mode, get the window image and display info for correct pixelation preview
        // NOTE: SelectedImageWidget may pre-scale the image before displaying it (see lines 362-390)
        // We need to calculate the TOTAL scaling from display coordinates to original image pixels
        let (window_image, window_display_info) = match &choice {
            Choice::Window(win_output, Some(win_idx)) => {
                if let Some(img) = toplevel_images
                    .get(win_output)
                    .and_then(|imgs| imgs.get(*win_idx))
                {
                    let orig_width = img.rgba.width() as f32;
                    let orig_height = img.rgba.height() as f32;
                    let output_width = output.logical_size.0 as f32;
                    let output_height = output.logical_size.1 as f32;

                    // Step 1: Calculate the pre-scaled thumbnail size (same logic as SelectedImageWidget)
                    let max_width = output_width * 0.85;
                    let max_height = output_height * 0.85;
                    let (thumb_width, thumb_height) = if orig_width > max_width || orig_height > max_height {
                        let pre_scale = (max_width / orig_width).min(max_height / orig_height);
                        (orig_width * pre_scale, orig_height * pre_scale)
                    } else {
                        (orig_width, orig_height)
                    };

                    // Step 2: Calculate display position and scale (centering the thumbnail)
                    let available_width = output_width - 20.0;
                    let available_height = output_height - 20.0;
                    let scale_x = available_width / thumb_width;
                    let scale_y = available_height / thumb_height;
                    let display_scale = scale_x.min(scale_y).min(1.0);

                    let display_width = thumb_width * display_scale;
                    let display_height = thumb_height * display_scale;
                    let x = (output_width - display_width) / 2.0;
                    let y = (output_height - display_height) / 2.0;

                    // Total scale from display coords to ORIGINAL image pixels
                    // display_coords → thumbnail_coords → original_coords
                    let display_to_original_scale = orig_width / display_width;
                    (
                        Some(&img.rgba),
                        Some((x, y, display_width, display_height, display_to_original_scale)),
                    )
                } else {
                    (None, None)
                }
            }
            _ => (None, None),
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
                // Check if a selection is complete (can show action buttons)
                let has_selection = match choice {
                    Choice::Rectangle(r, _) => r.dimensions().is_some(),
                    Choice::Window(_, Some(_)) => true,
                    Choice::Output(Some(_)) => true, // Only confirmed screen counts
                    _ => false,
                };

                // Determine if shape mode is active (any of the shape modes)
                let shape_mode_active = match primary_shape_tool {
                    ShapeTool::Arrow => arrow_mode,
                    ShapeTool::Circle => circle_mode,
                    ShapeTool::Rectangle => rect_outline_mode,
                };

                // Determine if redact mode is active (either redact or pixelate)
                let redact_mode_active = match primary_redact_tool {
                    crate::config::RedactTool::Redact => redact_mode,
                    crate::config::RedactTool::Pixelate => pixelate_mode,
                };

                build_toolbar(
                    choice.clone(),
                    output.name.clone(),
                    toolbar_position,
                    has_selection,
                    has_ocr_text,
                    qr_codes,
                    primary_shape_tool,
                    shape_mode_active,
                    shape_popup_open,
                    primary_redact_tool,
                    redact_mode_active,
                    redact_popup_open,
                    space_s,
                    space_xs,
                    space_xxs,
                    on_choice_change,
                    on_copy_to_clipboard,
                    on_save_to_pictures,
                    on_shape_popup_toggle.clone(),
                    on_open_shape_popup.clone(),
                    on_redact_popup_toggle.clone(),
                    on_open_redact_popup.clone(),
                    on_ocr.clone(),
                    on_ocr_copy.clone(),
                    on_qr.clone(),
                    on_qr_copy.clone(),
                    on_cancel,
                    &on_toolbar_position,
                    on_settings_toggle.clone(),
                    settings_drawer_open,
                    settings_drawer_open || shape_popup_open || redact_popup_open, // Keep toolbar opaque when any popup is open
                    output_count,
                )
            },
            choice,
            output_rect,
            output_name: output.name.clone(),
            arrows: arrows.to_vec(),
            arrow_mode,
            arrow_drawing,
            on_arrow_toggle: Some(on_arrow_toggle),
            on_arrow_start: Some(Box::new(on_arrow_start)),
            on_arrow_end: Some(Box::new(on_arrow_end)),
            circles: circles.to_vec(),
            circle_mode,
            circle_drawing,
            on_circle_toggle: Some(on_circle_toggle),
            rect_outlines: rect_outlines.to_vec(),
            rect_outline_mode,
            rect_outline_drawing,
            on_rect_outline_toggle: Some(on_rect_outline_toggle),
            redactions: redactions.to_vec(),
            redact_mode,
            redact_drawing,
            on_redact_toggle: Some(on_redact_toggle),
            on_redact_start: Some(Box::new(on_redact_start)),
            on_redact_end: Some(Box::new(on_redact_end)),
            pixelations: pixelations.to_vec(),
            annotations,
            pixelate_mode,
            pixelate_drawing,
            on_pixelate_toggle: Some(on_pixelate_toggle),
            on_pixelate_start: Some(Box::new(on_pixelate_start)),
            on_pixelate_end: Some(Box::new(on_pixelate_end)),
            on_clear_annotations: Some(on_clear_annotations.clone()),
            toolbar_position,
            on_toolbar_position: Some(Box::new(on_toolbar_position)),
            on_open_url: Some(Box::new(on_open_url)),
            settings_drawer_open,
            magnifier_enabled,
            on_settings_toggle: Some(on_settings_toggle),
            on_magnifier_toggle: Some(on_magnifier_toggle.clone()),
            save_location,
            on_save_location_pictures: Some(on_save_location_pictures.clone()),
            on_save_location_documents: Some(on_save_location_documents.clone()),
            copy_to_clipboard_on_save,
            on_copy_on_save_toggle: Some(on_copy_on_save_toggle.clone()),
            settings_drawer_element: if settings_drawer_open {
                Some(build_settings_drawer(
                    toolbar_position,
                    magnifier_enabled,
                    on_magnifier_toggle,
                    save_location,
                    on_save_location_pictures,
                    on_save_location_documents,
                    copy_to_clipboard_on_save,
                    on_copy_on_save_toggle,
                    space_s,
                    space_xs,
                ))
            } else {
                None
            },
            shape_popup_element: if shape_popup_open {
                Some(build_shape_popup(
                    primary_shape_tool,
                    shape_color,
                    shape_shadow,
                    has_any_annotations,
                    on_set_shape_tool(ShapeTool::Arrow),
                    on_set_shape_tool(ShapeTool::Circle),
                    on_set_shape_tool(ShapeTool::Rectangle),
                    &on_set_shape_color,
                    on_toggle_shape_shadow.clone(),
                    on_clear_annotations.clone(),
                    space_s,
                    space_xs,
                ))
            } else {
                None
            },
            redact_popup_element: if redact_popup_open {
                Some(build_redact_popup(
                    primary_redact_tool,
                    has_any_redactions,
                    pixelation_block_size,
                    on_set_redact_tool(crate::config::RedactTool::Redact),
                    on_set_redact_tool(crate::config::RedactTool::Pixelate),
                    on_set_pixelation_size,
                    on_save_pixelation_size,
                    on_clear_redactions.clone(),
                    space_s,
                    space_xs,
                ))
            } else {
                None
            },
            shapes_element: {
                // Canvas overlay handles preview rendering + input for circle/rect outline
                let program = ShapesOverlay {
                    selection_rect,
                    output_rect,
                    circles: circles.to_vec(),
                    rect_outlines: rect_outlines.to_vec(),
                    circle_mode,
                    rect_outline_mode,
                    circle_drawing,
                    rect_outline_drawing,
                    on_circle_start: Some(Box::new(on_circle_start)),
                    on_circle_end: Some(Box::new(on_circle_end)),
                    on_rect_start: Some(Box::new(on_rect_outline_start)),
                    on_rect_end: Some(Box::new(on_rect_outline_end)),
                    shape_color,
                    shape_shadow,
                };

                canvas::Canvas::new(program)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            },
            primary_shape_tool,
            shape_popup_open,
            shape_color,
            shape_shadow,
            on_shape_toggle: Some(on_shape_toggle),
            on_shape_popup_toggle: Some(on_shape_popup_toggle),
            on_open_shape_popup: Some(on_open_shape_popup),
            on_close_shape_popup: Some(on_close_shape_popup),
            on_set_shape_tool: Some(Box::new(on_set_shape_tool)),
            on_set_shape_color: Some(Box::new(on_set_shape_color)),
            on_toggle_shape_shadow: Some(on_toggle_shape_shadow),
            has_any_annotations,
            screenshot_image: &image.rgba,
            image_scale,
            pixelation_block_size,
            window_image,
            window_display_info,
            primary_redact_tool,
            redact_popup_open,
            on_redact_popup_toggle: Some(on_redact_popup_toggle),
            on_open_redact_popup: Some(on_open_redact_popup),
            on_close_redact_popup: Some(on_close_redact_popup),
            on_set_redact_tool: Some(Box::new(on_set_redact_tool)),
            on_clear_redactions: Some(on_clear_redactions),
            has_any_redactions,
            is_active_output,
            has_confirmed_selection,
            on_select_screen: Some(on_select_screen),
        }
    }
}

impl<'a, Msg: Clone> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for ScreenshotSelection<'a, Msg>
{
    fn children(&self) -> Vec<cosmic::iced_core::widget::Tree> {
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

    fn diff(&mut self, tree: &mut cosmic::iced_core::widget::Tree) {
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

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        translation: iced::Vector,
    ) -> Option<cosmic::iced_core::overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>> {
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

        // FIRST: Handle click-outside-to-close for settings drawer and shape selector
        // This must run before child widgets process the event
        if let cosmic::iced_core::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) = &event
            && let Some(pos) = cursor.position()
        {
            // Get the layout children to find popup bounds
            let layout_children: Vec<_> = layout.children().collect();

            // Handle shape selector popup click-outside (check first since it's on top)
            if self.shape_popup_open {
                // Shape selector is at index 4 if no settings drawer, or 5 if settings drawer exists
                let selector_idx = if self.settings_drawer_element.is_some() { 5 } else { 4 };
                let inside_selector = if layout_children.len() > selector_idx {
                    let selector_bounds = layout_children[selector_idx].bounds();
                    selector_bounds.contains(pos)
                } else {
                    false
                };

                // Also check if click is inside the toolbar (on the shape button itself)
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                // If clicked outside the selector and toolbar, just close popup (keep shape mode active)
                if !inside_selector && !inside_toolbar {
                    if let Some(ref on_close_shape_popup) = self.on_close_shape_popup {
                        shell.publish(on_close_shape_popup.clone());
                        return cosmic::iced_core::event::Status::Captured;
                    }
                }
            }

            // Handle redact popup click-outside
            if self.redact_popup_open {
                // Redact popup is after shape popup if present, after settings drawer if present
                let mut popup_idx = 4;
                if self.settings_drawer_element.is_some() { popup_idx += 1; }
                if self.shape_popup_element.is_some() { popup_idx += 1; }
                let inside_popup = if layout_children.len() > popup_idx {
                    let popup_bounds = layout_children[popup_idx].bounds();
                    popup_bounds.contains(pos)
                } else {
                    false
                };

                // Also check if click is inside the toolbar (on the redact button itself)
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                // If clicked outside the popup and toolbar, just close popup (keep redact mode active)
                if !inside_popup && !inside_toolbar {
                    if let Some(ref on_close_redact_popup) = self.on_close_redact_popup {
                        shell.publish(on_close_redact_popup.clone());
                        return cosmic::iced_core::event::Status::Captured;
                    }
                }
            }

            // Handle settings drawer click-outside
            if self.settings_drawer_open {
                // Drawer is at index 4 (after bg, fg, shapes, menu)
                let inside_drawer = if layout_children.len() > 4 {
                    let drawer_bounds = layout_children[4].bounds();
                    drawer_bounds.contains(pos)
                } else {
                    false
                };

                // Also check if click is inside the toolbar (index 3)
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                // If clicked outside the drawer and toolbar, close it
                if !inside_drawer && !inside_toolbar {
                    if let Some(ref on_settings_toggle) = self.on_settings_toggle {
                        shell.publish(on_settings_toggle.clone());
                        return cosmic::iced_core::event::Status::Captured;
                    }
                }
            }
        }

        // Block mouse events on non-active outputs when there's a confirmed selection
        // Don't auto-switch to screen mode - just block and show the overlay message
        if !self.is_active_output && self.has_confirmed_selection {
            // Block all mouse events on non-active outputs (the overlay shows a hint message)
            if matches!(&event, cosmic::iced_core::Event::Mouse(_)) {
                return cosmic::iced_core::event::Status::Captured;
            }
        }

        // Handle clicks on QR code URL open buttons (before child widgets)
        if let cosmic::iced_core::Event::Mouse(mouse_event) = &event
            && let Some(pos) = cursor.position()
            && matches!(mouse_event, MouseEvent::ButtonPressed(Button::Left))
            && let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect
        {
            let button_size = 28.0_f32;
            let padding = 8.0;

            for (x, y, content) in &self.qr_codes {
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

                // Check if click is inside button bounds
                if pos.x >= button_x
                    && pos.x <= button_x + button_size
                    && pos.y >= button_y
                    && pos.y <= button_y + button_size
                    && let Some(ref on_open_url) = self.on_open_url
                {
                    shell.publish(on_open_url(content.clone()));
                    return cosmic::iced_core::event::Status::Captured;
                }
            }
        }

        // Let child widgets handle the event (this includes toolbar buttons and drawer)
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
        if let cosmic::iced_core::Event::Mouse(mouse_event) = &event
            && let Some(pos) = cursor.position()
        {
            // Helper: check if position is on edge/corner of selection rectangle
            let on_edge_or_corner = |sel_x: f32, sel_y: f32, sel_w: f32, sel_h: f32| -> bool {
                const EDGE_THICKNESS: f32 = 12.0;
                const CORNER_SIZE: f32 = 16.0;
                
                // Check corners first (they take priority)
                let corners = [
                    (sel_x, sel_y),                           // NW
                    (sel_x + sel_w, sel_y),                   // NE
                    (sel_x, sel_y + sel_h),                   // SW
                    (sel_x + sel_w, sel_y + sel_h),           // SE
                ];
                for (cx, cy) in corners {
                    if (pos.x - cx).abs() < CORNER_SIZE && (pos.y - cy).abs() < CORNER_SIZE {
                        return true;
                    }
                }
                
                // Check edges
                // Top edge
                if pos.y >= sel_y - EDGE_THICKNESS / 2.0 && pos.y <= sel_y + EDGE_THICKNESS / 2.0
                    && pos.x >= sel_x && pos.x <= sel_x + sel_w {
                    return true;
                }
                // Bottom edge
                if pos.y >= sel_y + sel_h - EDGE_THICKNESS / 2.0 && pos.y <= sel_y + sel_h + EDGE_THICKNESS / 2.0
                    && pos.x >= sel_x && pos.x <= sel_x + sel_w {
                    return true;
                }
                // Left edge
                if pos.x >= sel_x - EDGE_THICKNESS / 2.0 && pos.x <= sel_x + EDGE_THICKNESS / 2.0
                    && pos.y >= sel_y && pos.y <= sel_y + sel_h {
                    return true;
                }
                // Right edge
                if pos.x >= sel_x + sel_w - EDGE_THICKNESS / 2.0 && pos.x <= sel_x + sel_w + EDGE_THICKNESS / 2.0
                    && pos.y >= sel_y && pos.y <= sel_y + sel_h {
                    return true;
                }
                
                false
            };
            
            // Check if click is on edge/corner - if so, toggle off annotation modes
            if let MouseEvent::ButtonPressed(Button::Left) = mouse_event {
                if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                    if on_edge_or_corner(sel_x, sel_y, sel_w, sel_h) {
                        // Toggle off any active annotation mode
                        if self.arrow_mode {
                            if let Some(ref on_arrow_toggle) = self.on_arrow_toggle {
                                shell.publish(on_arrow_toggle.clone());
                            }
                            return cosmic::iced_core::event::Status::Ignored; // Let rectangle handle it
                        }
                        if self.circle_mode {
                            if let Some(ref on_circle_toggle) = self.on_circle_toggle {
                                shell.publish(on_circle_toggle.clone());
                            }
                            return cosmic::iced_core::event::Status::Ignored;
                        }
                        if self.rect_outline_mode {
                            if let Some(ref on_rect_outline_toggle) = self.on_rect_outline_toggle {
                                shell.publish(on_rect_outline_toggle.clone());
                            }
                            return cosmic::iced_core::event::Status::Ignored;
                        }
                        if self.redact_mode {
                            if let Some(ref on_redact_toggle) = self.on_redact_toggle {
                                shell.publish(on_redact_toggle.clone());
                            }
                            return cosmic::iced_core::event::Status::Ignored;
                        }
                        if self.pixelate_mode {
                            if let Some(ref on_pixelate_toggle) = self.on_pixelate_toggle {
                                shell.publish(on_pixelate_toggle.clone());
                            }
                            return cosmic::iced_core::event::Status::Ignored;
                        }
                    }
                }
            }

            // Margin for annotation clamping (0 = clamp to exact edge)
            const ANNOTATION_MARGIN: f32 = 0.0;
            
            // Helper to clamp position within selection rect with margin
            let clamp_to_selection = |x: f32, y: f32, sel_x: f32, sel_y: f32, sel_w: f32, sel_h: f32| -> (f32, f32) {
                let min_x = sel_x + ANNOTATION_MARGIN;
                let max_x = sel_x + sel_w - ANNOTATION_MARGIN;
                let min_y = sel_y + ANNOTATION_MARGIN;
                let max_y = sel_y + sel_h - ANNOTATION_MARGIN;
                (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
            };
            
            // Check if position is inside the inner area (with margin)
            let inside_inner_selection = |sel_x: f32, sel_y: f32, sel_w: f32, sel_h: f32| -> bool {
                pos.x >= sel_x + ANNOTATION_MARGIN
                    && pos.x <= sel_x + sel_w - ANNOTATION_MARGIN
                    && pos.y >= sel_y + ANNOTATION_MARGIN
                    && pos.y <= sel_y + sel_h - ANNOTATION_MARGIN
            };

            // Handle arrow drawing mode - press to start, release to end
            if self.arrow_mode {
                // Check if position is inside inner selection rectangle (with margin)
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        inside_inner_selection(sel_x, sel_y, sel_w, sel_h)
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        // Start a new arrow on press (clamped to inner area)
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) = clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            if let Some(ref on_arrow_start) = self.on_arrow_start {
                                shell.publish(on_arrow_start(global_x, global_y));
                            }
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    MouseEvent::ButtonReleased(Button::Left) if self.arrow_drawing.is_some() => {
                        // Finish the arrow on release (clamped to inner area)
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) = clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            if let Some(ref on_arrow_end) = self.on_arrow_end {
                                shell.publish(on_arrow_end(global_x, global_y));
                            }
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    _ => {}
                }
            }

            // Handle redact drawing mode - press to start, release to end
            if self.redact_mode {
                // Check if position is inside inner selection rectangle (with margin)
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        inside_inner_selection(sel_x, sel_y, sel_w, sel_h)
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        // Start a new redaction on press (clamped to inner area)
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) = clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            if let Some(ref on_redact_start) = self.on_redact_start {
                                shell.publish(on_redact_start(global_x, global_y));
                            }
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    MouseEvent::ButtonReleased(Button::Left) if self.redact_drawing.is_some() => {
                        // Finish the redaction on release (clamped to inner area)
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) = clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            if let Some(ref on_redact_end) = self.on_redact_end {
                                shell.publish(on_redact_end(global_x, global_y));
                            }
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    _ => {}
                }
            }

            // Handle pixelate drawing mode - press to start, release to end
            if self.pixelate_mode {
                // Check if position is inside inner selection rectangle (with margin)
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        inside_inner_selection(sel_x, sel_y, sel_w, sel_h)
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        // Start a new pixelation on press (clamped to inner area)
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) = clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            if let Some(ref on_pixelate_start) = self.on_pixelate_start {
                                shell.publish(on_pixelate_start(global_x, global_y));
                            }
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    MouseEvent::ButtonReleased(Button::Left) if self.pixelate_drawing.is_some() => {
                        // Finish the pixelation on release (clamped to inner area)
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) = clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            if let Some(ref on_pixelate_end) = self.on_pixelate_end {
                                shell.publish(on_pixelate_end(global_x, global_y));
                            }
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    _ => {}
                }
            }

        // NOTE: circle/rect-outline drawing is handled by the Canvas (`shapes_element`)
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
        // Check if hovering over a QR URL button
        if let Some(pos) = cursor.position()
            && let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect
        {
            let button_size = 28.0_f32;
            let padding = 8.0;

            for (x, y, content) in &self.qr_codes {
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
                    return cosmic::iced_core::mouse::Interaction::Pointer;
                }
            }
        }

        let mut children: Vec<&Element<'_, Msg>> =
            vec![&self.bg_element, &self.fg_element, &self.shapes_element, &self.menu_element];
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
        let mut children: Vec<&Element<'_, Msg>> =
            vec![&self.bg_element, &self.fg_element, &self.shapes_element, &self.menu_element];
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
        let shapes_node = self
            .shapes_element
            .as_widget()
            .layout(&mut children[2], renderer, limits);
        let mut menu_node =
            self.menu_element
                .as_widget()
                .layout(&mut children[3], renderer, limits);
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

        let mut nodes = vec![bg_node, fg_node, shapes_node, menu_node.clone()];

        // Layout settings drawer if present
        if let Some(ref drawer) = self.settings_drawer_element {
            let mut drawer_node = drawer
                .as_widget()
                .layout(&mut children[4], renderer, limits);
            let drawer_bounds = drawer_node.bounds();
            let drawer_margin = 8.0_f32;

            // The settings button is second-to-last in the toolbar
            // We need to calculate its position relative to the toolbar
            // For horizontal toolbars: button is near the right edge
            // For vertical toolbars: button is near the bottom

            // Calculate settings button center position
            // The settings button is about 40px wide, positioned before the close button
            let button_offset = 40.0 + 8.0; // button width + half spacing

            let drawer_pos = match self.toolbar_position {
                ToolbarPosition::Bottom => {
                    // Drawer opens above the toolbar, centered on settings button
                    let settings_btn_x = menu_pos.x + menu_bounds.width - button_offset - 20.0;
                    Point {
                        x: (settings_btn_x - drawer_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - drawer_bounds.width - margin),
                        y: menu_pos.y - drawer_bounds.height - drawer_margin,
                    }
                }
                ToolbarPosition::Top => {
                    // Drawer opens below the toolbar
                    let settings_btn_x = menu_pos.x + menu_bounds.width - button_offset - 20.0;
                    Point {
                        x: (settings_btn_x - drawer_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - drawer_bounds.width - margin),
                        y: menu_pos.y + menu_bounds.height + drawer_margin,
                    }
                }
                ToolbarPosition::Left => {
                    // Drawer opens to the right of the toolbar
                    let settings_btn_y = menu_pos.y + menu_bounds.height - button_offset - 20.0;
                    Point {
                        x: menu_pos.x + menu_bounds.width + drawer_margin,
                        y: (settings_btn_y - drawer_bounds.height / 2.0)
                            .max(margin)
                            .min(limits.max().height - drawer_bounds.height - margin),
                    }
                }
                ToolbarPosition::Right => {
                    // Drawer opens to the left of the toolbar
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
            let child_idx = if self.settings_drawer_element.is_some() { 5 } else { 4 };
            let mut selector_node = selector
                .as_widget()
                .layout(&mut children[child_idx], renderer, limits);
            let selector_bounds = selector_node.bounds();
            let selector_margin = 4.0_f32;

            // Calculate shapes button position as a fraction of toolbar size
            // The shapes button is roughly 42% from the start of the toolbar (after position selector,
            // divider, and 3 selection buttons, at the start of the tool buttons section)
            // This approach is more robust than fixed pixel offsets
            let shapes_btn_fraction = 0.42_f32;

            let selector_pos = match self.toolbar_position {
                ToolbarPosition::Bottom => {
                    // Selector opens above the toolbar, aligned with shapes button
                    let shapes_btn_x = menu_pos.x + menu_bounds.width * shapes_btn_fraction;
                    Point {
                        x: (shapes_btn_x - selector_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - selector_bounds.width - margin),
                        y: menu_pos.y - selector_bounds.height - selector_margin,
                    }
                }
                ToolbarPosition::Top => {
                    // Selector opens below the toolbar
                    let shapes_btn_x = menu_pos.x + menu_bounds.width * shapes_btn_fraction;
                    Point {
                        x: (shapes_btn_x - selector_bounds.width / 2.0)
                            .max(margin)
                            .min(limits.max().width - selector_bounds.width - margin),
                        y: menu_pos.y + menu_bounds.height + selector_margin,
                    }
                }
                ToolbarPosition::Left => {
                    // Selector opens to the right of the toolbar
                    let shapes_btn_y = menu_pos.y + menu_bounds.height * shapes_btn_fraction;
                    Point {
                        x: menu_pos.x + menu_bounds.width + selector_margin,
                        y: (shapes_btn_y - selector_bounds.height / 2.0)
                            .max(margin)
                            .min(limits.max().height - selector_bounds.height - margin),
                    }
                }
                ToolbarPosition::Right => {
                    // Selector opens to the left of the toolbar
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
            if self.settings_drawer_element.is_some() { child_idx += 1; }
            if self.shape_popup_element.is_some() { child_idx += 1; }
            let mut popup_node = popup
                .as_widget()
                .layout(&mut children[child_idx], renderer, limits);
            let popup_bounds = popup_node.bounds();
            let popup_margin = 4.0_f32;

            // The redact button is roughly 52% from the start of the toolbar (after shapes button)
            let redact_btn_fraction = 0.52_f32;

            let popup_pos = match self.toolbar_position {
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

        let children = &[&self.bg_element, &self.fg_element, &self.shapes_element, &self.menu_element];
        let mut children_iter = layout.children().zip(children).enumerate();

        // Draw bg_element first (screenshot background)
        {
            let (i, (layout, child)) = children_iter.next().unwrap();
            let bg_tree = &tree.children[i];
            child
                .as_widget()
                .draw(bg_tree, renderer, theme, style, layout, cursor, viewport);
        }

        // If this is not the active output and there's a confirmed selection, draw a dark overlay
        if !self.is_active_output && self.has_confirmed_selection {
            let bounds = layout.bounds();
            
            // Use with_layer to ensure overlay is drawn on top of the background image
            renderer.with_layer(bounds, |renderer| {
                // Draw dark overlay
                let dark_overlay = cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.7);
                renderer.fill_quad(
                    cosmic::iced_core::renderer::Quad {
                        bounds,
                        border: Border::default(),
                        shadow: cosmic::iced_core::Shadow::default(),
                    },
                    Background::Color(dark_overlay),
                );

                // Draw a centered hint box with text
                let hint_text = "Press 'S' or Screen button to change selection";
                let font_size = 18.0;
                let box_width = 420.0_f32;
                let box_height = 50.0_f32;
                
                // Center the box in the screen
                let box_x = bounds.x + (bounds.width - box_width) / 2.0;
                let box_y = bounds.y + (bounds.height - box_height) / 2.0;
                
                let hint_box = cosmic::iced_core::Rectangle {
                    x: box_x,
                    y: box_y,
                    width: box_width,
                    height: box_height,
                };
                
                // Draw semi-transparent background for the hint box
                renderer.fill_quad(
                    cosmic::iced_core::renderer::Quad {
                        bounds: hint_box,
                        border: Border {
                            radius: 8.0.into(),
                            width: 0.0,
                            color: cosmic::iced::Color::TRANSPARENT,
                        },
                        shadow: cosmic::iced_core::Shadow::default(),
                    },
                    Background::Color(cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5)),
                );
                
                // Draw text centered in the hint box
                renderer.fill_text(
                    Text {
                        content: hint_text.to_string(),
                        bounds: cosmic::iced_core::Size::new(box_width, box_height),
                        size: cosmic::iced_core::Pixels(font_size),
                        line_height: cosmic::iced_core::text::LineHeight::Relative(1.0),
                        font: cosmic::iced_core::Font {
                            weight: cosmic::iced_core::font::Weight::Medium,
                            ..Default::default()
                        },
                        horizontal_alignment: cosmic::iced::alignment::Horizontal::Center,
                        vertical_alignment: cosmic::iced::alignment::Vertical::Center,
                        shaping: cosmic::iced_core::text::Shaping::Advanced,
                        wrapping: cosmic::iced_core::text::Wrapping::None,
                    },
                    cosmic::iced_core::Point::new(box_x, box_y),
                    cosmic::iced::Color::WHITE,
                    hint_box,
                );
            });

            // Don't draw any more elements on non-active output
            return;
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

        // ========== REDACTIONS AND PIXELATIONS (drawn BEFORE shapes, in correct order) ==========
        // Draw redactions and pixelations in the order they appear in the annotations array
        let redact_color = cosmic::iced::Color::BLACK;
        for annotation in self.annotations {
            match annotation {
                Annotation::Redact(redact) => {
                    let x1 = redact.x - self.output_rect.left as f32;
                    let y1 = redact.y - self.output_rect.top as f32;
                    let x2 = redact.x2 - self.output_rect.left as f32;
                    let y2 = redact.y2 - self.output_rect.top as f32;
                    let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
                    let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
                    let rect = cosmic::iced_core::Rectangle {
                        x: min_x,
                        y: min_y,
                        width: max_x - min_x,
                        height: max_y - min_y,
                    };
                    renderer.with_layer(*viewport, |renderer| {
                        renderer.fill_quad(
                            cosmic::iced_core::renderer::Quad {
                                bounds: rect,
                                border: Border::default(),
                                shadow: cosmic::iced_core::Shadow::default(),
                            },
                            Background::Color(redact_color),
                        );
                    });
                }
                Annotation::Pixelate(pixelate) => {
                    let x1 = pixelate.x - self.output_rect.left as f32;
                    let y1 = pixelate.y - self.output_rect.top as f32;
                    let x2 = pixelate.x2 - self.output_rect.left as f32;
                    let y2 = pixelate.y2 - self.output_rect.top as f32;
                    let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
                    let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

                    // Check if we're in window mode
                    if let (Some(win_img), Some((win_x, win_y, _win_w, _win_h, display_to_img_scale))) =
                        (self.window_image, self.window_display_info)
                    {
                        // Window mode: sample from window image
                        let block_size_display = pixelate.block_size as f32;

                        renderer.with_layer(*viewport, |renderer| {
                            let mut y = min_y;
                            while y < max_y {
                                let mut x = min_x;
                                let block_h = block_size_display.min(max_y - y);
                                while x < max_x {
                                    let block_w = block_size_display.min(max_x - x);
                                    // Convert from screen coords to window image coords
                                    let win_rel_x = x - win_x;
                                    let win_rel_y = y - win_y;
                                    let img_x = (win_rel_x * display_to_img_scale).round() as i32;
                                    let img_y = (win_rel_y * display_to_img_scale).round() as i32;
                                    let img_x2 = ((win_rel_x + block_w) * display_to_img_scale).round() as i32;
                                    let img_y2 = ((win_rel_y + block_h) * display_to_img_scale).round() as i32;

                                    // Skip if outside window image bounds
                                    if img_x >= 0 && img_y >= 0 && img_x2 > 0 && img_y2 > 0 {
                                        let img_x = (img_x as u32).min(win_img.width().saturating_sub(1));
                                        let img_y = (img_y as u32).min(win_img.height().saturating_sub(1));
                                        let img_x2 = (img_x2 as u32).min(win_img.width());
                                        let img_y2 = (img_y2 as u32).min(win_img.height());
                                        let mut total_r: u64 = 0;
                                        let mut total_g: u64 = 0;
                                        let mut total_b: u64 = 0;
                                        let mut pixel_count: u64 = 0;
                                        for py in img_y..img_y2 {
                                            for px in img_x..img_x2 {
                                                let pixel = win_img.get_pixel(px, py);
                                                total_r += pixel[0] as u64;
                                                total_g += pixel[1] as u64;
                                                total_b += pixel[2] as u64;
                                                pixel_count += 1;
                                            }
                                        }
                                        if pixel_count > 0 {
                                            let color = cosmic::iced::Color::from_rgb8(
                                                (total_r / pixel_count) as u8,
                                                (total_g / pixel_count) as u8,
                                                (total_b / pixel_count) as u8,
                                            );
                                            renderer.fill_quad(
                                                cosmic::iced_core::renderer::Quad {
                                                    bounds: cosmic::iced_core::Rectangle {
                                                        x,
                                                        y,
                                                        width: block_w,
                                                        height: block_h,
                                                    },
                                                    border: Border::default(),
                                                    shadow: cosmic::iced_core::Shadow::default(),
                                                },
                                                Background::Color(color),
                                            );
                                        }
                                    }
                                    x += block_w;
                                }
                                y += block_h;
                            }
                        });
                    } else {
                        // Regular mode: sample from screenshot image
                        let block_size_logical = pixelate.block_size as f32 / self.image_scale;

                        renderer.with_layer(*viewport, |renderer| {
                            let mut y = min_y;
                            while y < max_y {
                                let mut x = min_x;
                                let block_h = block_size_logical.min(max_y - y);
                                while x < max_x {
                                    let block_w = block_size_logical.min(max_x - x);
                                    let img_x = (x * self.image_scale).round() as u32;
                                    let img_y = (y * self.image_scale).round() as u32;
                                    let img_x2 = ((x + block_w) * self.image_scale).round() as u32;
                                    let img_y2 = ((y + block_h) * self.image_scale).round() as u32;
                                    let img_x = img_x.min(self.screenshot_image.width().saturating_sub(1));
                                    let img_y = img_y.min(self.screenshot_image.height().saturating_sub(1));
                                    let img_x2 = img_x2.min(self.screenshot_image.width());
                                    let img_y2 = img_y2.min(self.screenshot_image.height());
                                    let mut total_r: u64 = 0;
                                    let mut total_g: u64 = 0;
                                    let mut total_b: u64 = 0;
                                    let mut pixel_count: u64 = 0;
                                    for py in img_y..img_y2 {
                                        for px in img_x..img_x2 {
                                            let pixel = self.screenshot_image.get_pixel(px, py);
                                            total_r += pixel[0] as u64;
                                            total_g += pixel[1] as u64;
                                            total_b += pixel[2] as u64;
                                            pixel_count += 1;
                                        }
                                    }
                                    if pixel_count > 0 {
                                        let color = cosmic::iced::Color::from_rgb8(
                                            (total_r / pixel_count) as u8,
                                            (total_g / pixel_count) as u8,
                                            (total_b / pixel_count) as u8,
                                        );
                                        renderer.fill_quad(
                                            cosmic::iced_core::renderer::Quad {
                                                bounds: cosmic::iced_core::Rectangle {
                                                    x,
                                                    y,
                                                    width: block_w,
                                                    height: block_h,
                                                },
                                                border: Border::default(),
                                                shadow: cosmic::iced_core::Shadow::default(),
                                            },
                                            Background::Color(color),
                                        );
                                    }
                                    x += block_w;
                                }
                                y += block_h;
                            }
                        });
                    }
                }
                // Skip other annotation types - they're drawn later
                _ => {}
            }
        }

        // Draw pixelation preview (currently being drawn)
        if let Some((start_x, start_y)) = self.pixelate_drawing
            && let Some(cursor_pos) = cursor.position()
        {
            let local_start_x = start_x - self.output_rect.left as f32;
            let local_start_y = start_y - self.output_rect.top as f32;
            let end_x = cursor_pos.x;
            let end_y = cursor_pos.y;
            let (min_x, max_x) = if local_start_x < end_x {
                (local_start_x, end_x)
            } else {
                (end_x, local_start_x)
            };
            let (min_y, max_y) = if local_start_y < end_y {
                (local_start_y, end_y)
            } else {
                (end_y, local_start_y)
            };

            // Check if we're in window mode with window image data
            if let (Some(win_img), Some((win_x, win_y, _win_w, _win_h, display_to_img_scale))) =
                (self.window_image, self.window_display_info)
            {
                // Window mode: sample from window image with proper coordinate transformation
                // Block size in display coordinates (same as pixelation_block_size since it's in logical units)
                let block_size_display = self.pixelation_block_size as f32;

                renderer.with_layer(*viewport, |renderer| {
                    let mut y = min_y;
                    while y < max_y {
                        let mut x = min_x;
                        let block_h = block_size_display.min(max_y - y);
                        while x < max_x {
                            let block_w = block_size_display.min(max_x - x);
                            // Convert from screen coords to window image coords
                            let win_rel_x = x - win_x;
                            let win_rel_y = y - win_y;
                            let img_x = (win_rel_x * display_to_img_scale).round() as i32;
                            let img_y = (win_rel_y * display_to_img_scale).round() as i32;
                            let img_x2 = ((win_rel_x + block_w) * display_to_img_scale).round() as i32;
                            let img_y2 = ((win_rel_y + block_h) * display_to_img_scale).round() as i32;

                            // Skip if outside window image bounds
                            if img_x >= 0 && img_y >= 0 && img_x2 > 0 && img_y2 > 0 {
                                let img_x = (img_x as u32).min(win_img.width().saturating_sub(1));
                                let img_y = (img_y as u32).min(win_img.height().saturating_sub(1));
                                let img_x2 = (img_x2 as u32).min(win_img.width());
                                let img_y2 = (img_y2 as u32).min(win_img.height());
                                let mut total_r: u64 = 0;
                                let mut total_g: u64 = 0;
                                let mut total_b: u64 = 0;
                                let mut pixel_count: u64 = 0;
                                for py in img_y..img_y2 {
                                    for px in img_x..img_x2 {
                                        let pixel = win_img.get_pixel(px, py);
                                        total_r += pixel[0] as u64;
                                        total_g += pixel[1] as u64;
                                        total_b += pixel[2] as u64;
                                        pixel_count += 1;
                                    }
                                }
                                if pixel_count > 0 {
                                    let color = cosmic::iced::Color::from_rgb8(
                                        (total_r / pixel_count) as u8,
                                        (total_g / pixel_count) as u8,
                                        (total_b / pixel_count) as u8,
                                    );
                                    renderer.fill_quad(
                                        cosmic::iced_core::renderer::Quad {
                                            bounds: cosmic::iced_core::Rectangle {
                                                x,
                                                y,
                                                width: block_w,
                                                height: block_h,
                                            },
                                            border: Border::default(),
                                            shadow: cosmic::iced_core::Shadow::default(),
                                        },
                                        Background::Color(color),
                                    );
                                }
                            }
                            x += block_w;
                        }
                        y += block_h;
                    }

                    // Draw border
                    renderer.fill_quad(
                        cosmic::iced_core::renderer::Quad {
                            bounds: cosmic::iced_core::Rectangle {
                                x: min_x,
                                y: min_y,
                                width: max_x - min_x,
                                height: max_y - min_y,
                            },
                            border: Border {
                                color: cosmic::iced::Color::WHITE,
                                width: 1.0,
                                radius: cosmic::iced_core::border::Radius::from(0.0),
                            },
                            shadow: cosmic::iced_core::Shadow::default(),
                        },
                        Background::Color(cosmic::iced::Color::TRANSPARENT),
                    );
                });
            } else {
                // Regular mode (rectangle/output): sample from screenshot image
                let block_size_logical = self.pixelation_block_size as f32 / self.image_scale;

                renderer.with_layer(*viewport, |renderer| {
                    let mut y = min_y;
                    while y < max_y {
                        let mut x = min_x;
                        let block_h = block_size_logical.min(max_y - y);
                        while x < max_x {
                            let block_w = block_size_logical.min(max_x - x);
                            let img_x = (x * self.image_scale).round() as u32;
                            let img_y = (y * self.image_scale).round() as u32;
                            let img_x2 = ((x + block_w) * self.image_scale).round() as u32;
                            let img_y2 = ((y + block_h) * self.image_scale).round() as u32;
                            let img_x = img_x.min(self.screenshot_image.width().saturating_sub(1));
                            let img_y = img_y.min(self.screenshot_image.height().saturating_sub(1));
                            let img_x2 = img_x2.min(self.screenshot_image.width());
                            let img_y2 = img_y2.min(self.screenshot_image.height());
                            let mut total_r: u64 = 0;
                            let mut total_g: u64 = 0;
                            let mut total_b: u64 = 0;
                            let mut pixel_count: u64 = 0;
                            for py in img_y..img_y2 {
                                for px in img_x..img_x2 {
                                    let pixel = self.screenshot_image.get_pixel(px, py);
                                    total_r += pixel[0] as u64;
                                    total_g += pixel[1] as u64;
                                    total_b += pixel[2] as u64;
                                    pixel_count += 1;
                                }
                            }
                            if pixel_count > 0 {
                                let color = cosmic::iced::Color::from_rgb8(
                                    (total_r / pixel_count) as u8,
                                    (total_g / pixel_count) as u8,
                                    (total_b / pixel_count) as u8,
                                );
                                renderer.fill_quad(
                                    cosmic::iced_core::renderer::Quad {
                                        bounds: cosmic::iced_core::Rectangle {
                                            x,
                                            y,
                                            width: block_w,
                                            height: block_h,
                                        },
                                        border: Border::default(),
                                        shadow: cosmic::iced_core::Shadow::default(),
                                    },
                                    Background::Color(color),
                                );
                            }
                            x += block_w;
                        }
                        y += block_h;
                    }

                    // Draw border
                    renderer.fill_quad(
                        cosmic::iced_core::renderer::Quad {
                            bounds: cosmic::iced_core::Rectangle {
                                x: min_x,
                                y: min_y,
                                width: max_x - min_x,
                                height: max_y - min_y,
                            },
                            border: Border {
                                color: cosmic::iced::Color::WHITE,
                                width: 1.0,
                                radius: cosmic::iced_core::border::Radius::from(0.0),
                            },
                            shadow: cosmic::iced_core::Shadow::default(),
                        },
                        Background::Color(cosmic::iced::Color::TRANSPARENT),
                    );
                });
            }
        }

        // Draw redaction preview (currently being drawn)
        if let Some((start_x, start_y)) = self.redact_drawing
            && let Some(cursor_pos) = cursor.position()
        {
            let local_start_x = start_x - self.output_rect.left as f32;
            let local_start_y = start_y - self.output_rect.top as f32;
            let end_x = cursor_pos.x;
            let end_y = cursor_pos.y;
            let (min_x, max_x) = if local_start_x < end_x {
                (local_start_x, end_x)
            } else {
                (end_x, local_start_x)
            };
            let (min_y, max_y) = if local_start_y < end_y {
                (local_start_y, end_y)
            } else {
                (end_y, local_start_y)
            };
            renderer.with_layer(*viewport, |renderer| {
                renderer.fill_quad(
                    cosmic::iced_core::renderer::Quad {
                        bounds: cosmic::iced_core::Rectangle {
                            x: min_x,
                            y: min_y,
                            width: max_x - min_x,
                            height: max_y - min_y,
                        },
                        border: Border {
                            color: cosmic::iced::Color::WHITE,
                            width: 1.0,
                            radius: cosmic::iced_core::border::Radius::from(0.0),
                        },
                        shadow: cosmic::iced_core::Shadow::default(),
                    },
                    Background::Color(cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.7)),
                );
            });
        }
        // ========== END REDACTIONS AND PIXELATIONS ==========

        // Draw shapes canvas overlay (circle/rectangle outlines) - AFTER redactions
        if let Some((i, (layout, child))) = children_iter.next() {
            renderer.with_layer(layout.bounds(), |renderer| {
                let tree = &tree.children[i];
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        }

        /// Build an arrow mesh using lines with rounded caps (shaft line + 2 angled head lines)
        fn build_arrow_lines_mesh(
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

            let mut inner = color;
            inner.a = inner.a.clamp(0.0, 1.0);
            let packed_inner = pack(inner);

            let mut outer = color;
            outer.a = 0.0;
            let packed_outer = pack(outer);

            let radius = thickness / 2.0;
            let feather = 2.0_f32; // Anti-aliasing feather width

            let mut vertices = Vec::new();
            let mut indices = Vec::new();

            // Helper to build a line segment
            fn add_line_segment(
                vertices: &mut Vec<SolidVertex2D>,
                indices: &mut Vec<u32>,
                x0: f32, y0: f32, x1: f32, y1: f32,
                radius: f32, feather: f32,
                packed_inner: Packed, packed_outer: Packed,
            ) {
                let ldx = x1 - x0;
                let ldy = y1 - y0;
                let llen = (ldx * ldx + ldy * ldy).sqrt();
                if llen < 0.1 {
                    return;
                }
                let lnx = ldx / llen;
                let lny = ldy / llen;

                // Perpendicular
                let px = -lny;
                let py = lnx;

                let base_idx = vertices.len() as u32;

                // Inner quad (solid core)
                let inner_r = radius;
                let outer_r = radius + feather;

                // Add inner quad vertices
                vertices.push(SolidVertex2D {
                    position: [x0 + px * inner_r, y0 + py * inner_r],
                    color: packed_inner,
                });
                vertices.push(SolidVertex2D {
                    position: [x0 - px * inner_r, y0 - py * inner_r],
                    color: packed_inner,
                });
                vertices.push(SolidVertex2D {
                    position: [x1 - px * inner_r, y1 - py * inner_r],
                    color: packed_inner,
                });
                vertices.push(SolidVertex2D {
                    position: [x1 + px * inner_r, y1 + py * inner_r],
                    color: packed_inner,
                });

                // Add outer quad vertices (for feathering)
                vertices.push(SolidVertex2D {
                    position: [x0 + px * outer_r, y0 + py * outer_r],
                    color: packed_outer,
                });
                vertices.push(SolidVertex2D {
                    position: [x0 - px * outer_r, y0 - py * outer_r],
                    color: packed_outer,
                });
                vertices.push(SolidVertex2D {
                    position: [x1 - px * outer_r, y1 - py * outer_r],
                    color: packed_outer,
                });
                vertices.push(SolidVertex2D {
                    position: [x1 + px * outer_r, y1 + py * outer_r],
                    color: packed_outer,
                });

                // Inner quad triangles
                indices.extend_from_slice(&[base_idx, base_idx + 1, base_idx + 2]);
                indices.extend_from_slice(&[base_idx, base_idx + 2, base_idx + 3]);

                // Feather band triangles (+ side)
                indices.extend_from_slice(&[base_idx + 4, base_idx, base_idx + 3]);
                indices.extend_from_slice(&[base_idx + 4, base_idx + 3, base_idx + 7]);

                // Feather band triangles (- side)
                indices.extend_from_slice(&[base_idx + 5, base_idx + 6, base_idx + 2]);
                indices.extend_from_slice(&[base_idx + 5, base_idx + 2, base_idx + 1]);
            }

            // Helper to add a circle (rounded cap)
            fn add_circle(
                vertices: &mut Vec<SolidVertex2D>,
                indices: &mut Vec<u32>,
                cx: f32, cy: f32,
                radius: f32, feather: f32,
                packed_inner: Packed, packed_outer: Packed,
            ) {
                let base_idx = vertices.len() as u32;
                let segments = 12;

                // Center vertex
                vertices.push(SolidVertex2D {
                    position: [cx, cy],
                    color: packed_inner,
                });

                // Inner ring vertices
                for i in 0..segments {
                    let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
                    vertices.push(SolidVertex2D {
                        position: [cx + radius * angle.cos(), cy + radius * angle.sin()],
                        color: packed_inner,
                    });
                }

                // Outer ring vertices (for feathering)
                let outer_r = radius + feather;
                for i in 0..segments {
                    let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
                    vertices.push(SolidVertex2D {
                        position: [cx + outer_r * angle.cos(), cy + outer_r * angle.sin()],
                        color: packed_outer,
                    });
                }

                // Inner circle triangles (center to inner ring)
                for i in 0..segments {
                    let next = (i + 1) % segments;
                    indices.push(base_idx);
                    indices.push(base_idx + 1 + i as u32);
                    indices.push(base_idx + 1 + next as u32);
                }

                // Feather ring triangles (inner ring to outer ring)
                for i in 0..segments {
                    let next = (i + 1) % segments;
                    let inner_i = base_idx + 1 + i as u32;
                    let inner_next = base_idx + 1 + next as u32;
                    let outer_i = base_idx + 1 + segments as u32 + i as u32;
                    let outer_next = base_idx + 1 + segments as u32 + next as u32;

                    indices.push(inner_i);
                    indices.push(outer_i);
                    indices.push(outer_next);
                    indices.push(inner_i);
                    indices.push(outer_next);
                    indices.push(inner_next);
                }
            }

            // Draw the shaft line from start to end
            add_line_segment(&mut vertices, &mut indices, start_x, start_y, end_x, end_y, radius, feather, packed_inner, packed_outer);

            // Add rounded caps at start and end
            add_circle(&mut vertices, &mut indices, start_x, start_y, radius, feather, packed_inner, packed_outer);
            add_circle(&mut vertices, &mut indices, end_x, end_y, radius, feather, packed_inner, packed_outer);

            // Arrowhead: two angled lines at the tip
            let angle = 35.0_f32.to_radians();
            let cos_a = angle.cos();
            let sin_a = angle.sin();

            // First head line (rotated clockwise from arrow direction)
            let head1_dx = -nx * cos_a - (-ny) * sin_a;
            let head1_dy = -nx * sin_a + (-ny) * cos_a;
            let head1_end_x = end_x + head1_dx * head_size;
            let head1_end_y = end_y + head1_dy * head_size;
            add_line_segment(&mut vertices, &mut indices, end_x, end_y, head1_end_x, head1_end_y, radius, feather, packed_inner, packed_outer);
            add_circle(&mut vertices, &mut indices, head1_end_x, head1_end_y, radius, feather, packed_inner, packed_outer);

            // Second head line (rotated counter-clockwise)
            let head2_dx = -nx * cos_a + (-ny) * sin_a;
            let head2_dy = -nx * (-sin_a) + (-ny) * cos_a;
            let head2_end_x = end_x + head2_dx * head_size;
            let head2_end_y = end_y + head2_dy * head_size;
            add_line_segment(&mut vertices, &mut indices, end_x, end_y, head2_end_x, head2_end_y, radius, feather, packed_inner, packed_outer);
            add_circle(&mut vertices, &mut indices, head2_end_x, head2_end_y, radius, feather, packed_inner, packed_outer);

            Some((vertices, indices))
        }

        // NOTE: Redactions and pixelations are now drawn BEFORE shapes_element (above)

        // Draw arrows on top of shapes using meshes
        let border_color = cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.9);
        let arrow_thickness = 4.0_f32;
        let head_size = 16.0_f32;
        let outline_px = 1.0_f32;

        for arrow in &self.arrows {
            // Use per-arrow color
            let arrow_color: cosmic::iced::Color = arrow.color.into();
            
            // Convert global coordinates to widget-local
            let start_x = arrow.start_x - self.output_rect.left as f32;
            let start_y = arrow.start_y - self.output_rect.top as f32;
            let end_x = arrow.end_x - self.output_rect.left as f32;
            let end_y = arrow.end_y - self.output_rect.top as f32;

            // Border/shadow first, then main arrow
            if arrow.shadow {
                if let Some((vertices, indices)) = build_arrow_lines_mesh(
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    border_color,
                    arrow_thickness + 2.0 * outline_px,
                    head_size + outline_px,
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

            if let Some((vertices, indices)) = build_arrow_lines_mesh(
                start_x,
                start_y,
                end_x,
                end_y,
                arrow_color,
                arrow_thickness,
                head_size,
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

        // Draw arrow currently being drawn (preview) - uses current shape_color
        let current_shape_color: cosmic::iced::Color = self.shape_color.into();
        if let Some((start_x, start_y)) = self.arrow_drawing
            && let Some(cursor_pos) = cursor.position()
        {
            let local_start_x = start_x - self.output_rect.left as f32;
            let local_start_y = start_y - self.output_rect.top as f32;
            let end_x = cursor_pos.x;
            let end_y = cursor_pos.y;

            let mut preview_color = current_shape_color;
            preview_color.a = 0.7;
            let preview_border_color = cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6);

            if self.shape_shadow {
                if let Some((vertices, indices)) = build_arrow_lines_mesh(
                    local_start_x,
                    local_start_y,
                    end_x,
                    end_y,
                    preview_border_color,
                    arrow_thickness + 2.0 * outline_px,
                    head_size + outline_px,
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

            if let Some((vertices, indices)) = build_arrow_lines_mesh(
                local_start_x,
                local_start_y,
                end_x,
                end_y,
                preview_color,
                arrow_thickness,
                head_size,
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
                let button_size = 28.0_f32;

                for (x, y, content) in &self.qr_codes {
                    let font_size = 14.0_f32;
                    let padding = 8.0;
                    let content_is_url = is_url(content);

                    // Calculate max label width based on selection rectangle
                    // Reserve space for button if it's a URL
                    let button_space = if content_is_url {
                        button_size + padding
                    } else {
                        0.0
                    };
                    let max_label_width = (sel_w - padding * 4.0 - button_space).clamp(80.0, 400.0);

                    // Estimate number of lines for wrapped text
                    let chars_per_line = (max_label_width / (font_size * 0.55)).max(10.0) as usize;
                    let num_lines = ((content.len() / chars_per_line).max(1) + 1).min(6); // Cap at 6 lines
                    let text_height = (num_lines as f32 * font_size * 1.3).min(sel_h * 0.6);

                    let bg_width = max_label_width + padding * 2.0 + button_space;
                    let bg_height = text_height.max(button_size) + padding * 2.0;

                    // Position centered on QR location, but clamp to selection bounds
                    let mut label_x = *x - bg_width / 2.0;
                    let mut label_y = *y - bg_height / 2.0;

                    // Clamp to selection rectangle
                    label_x = label_x
                        .max(sel_x + padding)
                        .min(sel_x + sel_w - bg_width - padding);
                    label_y = label_y
                        .max(sel_y + padding)
                        .min(sel_y + sel_h - bg_height - padding);

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

                        // Draw "open URL" button if content is a URL
                        if content_is_url {
                            let button_x = bg_rect.x + bg_width - padding - button_size;
                            let button_y = bg_rect.y + (bg_height - button_size) / 2.0;

                            let button_rect = cosmic::iced_core::Rectangle {
                                x: button_x,
                                y: button_y,
                                width: button_size,
                                height: button_size,
                            };

                            // Draw button background
                            renderer.fill_quad(
                                cosmic::iced_core::renderer::Quad {
                                    bounds: button_rect,
                                    border: Border {
                                        radius: (button_size / 4.0).into(),
                                        width: 1.0,
                                        color: accent_color,
                                    },
                                    shadow: cosmic::iced_core::Shadow::default(),
                                },
                                Background::Color(accent_color),
                            );

                            // Draw a simple arrow/external link icon (→)
                            let icon_text = Text {
                                content: "🔗".to_string(),
                                bounds: Size::new(button_size, button_size),
                                size: cosmic::iced::Pixels(16.0),
                                line_height: cosmic::iced_core::text::LineHeight::default(),
                                font: cosmic::iced::Font::default(),
                                horizontal_alignment: alignment::Horizontal::Center,
                                vertical_alignment: alignment::Vertical::Center,
                                shaping: cosmic::iced_core::text::Shaping::Advanced,
                                wrapping: cosmic::iced_core::text::Wrapping::None,
                            };

                            renderer.fill_text(
                                icon_text,
                                Point::new(
                                    button_x + button_size / 2.0,
                                    button_y + button_size / 2.0,
                                ),
                                cosmic::iced::Color::WHITE,
                                *viewport,
                            );
                        }
                    });
                }
            }
        }

        // Show OCR status indicator (only when downloading, running, or error - not when done or idle)
        let show_ocr_status = matches!(
            &self.ocr_status,
            OcrStatus::DownloadingModels | OcrStatus::Running | OcrStatus::Error(_)
        );
        if show_ocr_status {
            let status_text = match &self.ocr_status {
                OcrStatus::DownloadingModels => "Downloading OCR models...".to_string(),
                OcrStatus::Running => "Running OCR...".to_string(),
                OcrStatus::Error(err) => format!(
                    "OCR error: {}",
                    if err.len() > 40 {
                        format!("{}...", &err[..37])
                    } else {
                        err.clone()
                    }
                ),
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
                cosmic::iced::Color::from_rgb(0.2, 0.6, 0.9), // Blue
                cosmic::iced::Color::from_rgb(0.9, 0.3, 0.3), // Red
                cosmic::iced::Color::from_rgb(0.3, 0.8, 0.3), // Green
                cosmic::iced::Color::from_rgb(0.9, 0.6, 0.2), // Orange
                cosmic::iced::Color::from_rgb(0.7, 0.3, 0.9), // Purple
                cosmic::iced::Color::from_rgb(0.2, 0.8, 0.8), // Cyan
                cosmic::iced::Color::from_rgb(0.9, 0.9, 0.2), // Yellow
                cosmic::iced::Color::from_rgb(0.9, 0.4, 0.7), // Pink
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

        // ========== DRAW SELECTION FRAME ON TOP OF ALL ANNOTATIONS ==========
        // Draw the selection rectangle border and corner handles after all annotations
        // so they're always visible and clickable
        // Only draw for rectangle and window modes (not screen mode where selection = full screen)
        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
            // Skip if selection covers the entire output (screen mode)
            let output_width = (self.output_rect.right - self.output_rect.left) as f32;
            let output_height = (self.output_rect.bottom - self.output_rect.top) as f32;
            let is_full_screen = sel_x == 0.0 && sel_y == 0.0 
                && (sel_w - output_width).abs() < 1.0 
                && (sel_h - output_height).abs() < 1.0;
            
            if sel_w > 0.0 && sel_h > 0.0 && !is_full_screen {
                let cosmic_theme = theme.cosmic();
                let accent = cosmic::iced::Color::from(cosmic_theme.accent_color());
                let radius_s = cosmic_theme.radius_s();
                
                // Selection border (2px accent color)
                let sel_rect = cosmic::iced_core::Rectangle {
                    x: sel_x,
                    y: sel_y,
                    width: sel_w,
                    height: sel_h,
                };
                renderer.fill_quad(
                    cosmic::iced_core::renderer::Quad {
                        bounds: sel_rect,
                        border: Border {
                            radius: 0.0.into(),
                            width: 2.0,
                            color: accent,
                        },
                        shadow: cosmic::iced_core::Shadow::default(),
                    },
                    Background::Color(cosmic::iced::Color::TRANSPARENT),
                );
                
                // Corner handles (circles at each corner)
                let corner_size = 12.0_f32;
                let corners = [
                    (sel_x, sel_y),                    // NW
                    (sel_x + sel_w, sel_y),            // NE
                    (sel_x, sel_y + sel_h),            // SW
                    (sel_x + sel_w, sel_y + sel_h),    // SE
                ];
                for (cx, cy) in corners {
                    let bounds = cosmic::iced_core::Rectangle {
                        x: cx - corner_size / 2.0,
                        y: cy - corner_size / 2.0,
                        width: corner_size,
                        height: corner_size,
                    };
                    renderer.fill_quad(
                        cosmic::iced_core::renderer::Quad {
                            bounds,
                            border: Border {
                                radius: radius_s.into(),
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

        // Draw settings drawer if present
        if let Some(ref drawer) = self.settings_drawer_element {
            // Get the drawer layout (4th child)
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

        // Draw shape selector popup if present
        if let Some(ref selector) = self.shape_popup_element {
            let layout_children: Vec<_> = layout.children().collect();
            let selector_idx = if self.settings_drawer_element.is_some() { 5 } else { 4 };
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

        // Draw redact popup if present
        if let Some(ref popup) = self.redact_popup_element {
            let layout_children: Vec<_> = layout.children().collect();
            let mut popup_idx = 4;
            if self.settings_drawer_element.is_some() { popup_idx += 1; }
            if self.shape_popup_element.is_some() { popup_idx += 1; }
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

    fn drag_destinations(
        &self,
        state: &cosmic::iced_core::widget::Tree,
        layout: cosmic::iced_core::Layout<'_>,
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

impl<'a, Message> From<ScreenshotSelection<'a, Message>> for cosmic::Element<'a, Message>
where
    Message: 'static + Clone,
{
    fn from(w: ScreenshotSelection<'a, Message>) -> cosmic::Element<'a, Message> {
        Element::new(w)
    }
}

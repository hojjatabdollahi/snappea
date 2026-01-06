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
        color::pack,
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
        ArrowAnnotation, Choice, CircleOutlineAnnotation, DetectedQrCode, OcrStatus, OcrTextOverlay,
        PixelateAnnotation, Rect, RectOutlineAnnotation, RedactAnnotation, ScreenshotImage,
        ToolbarPosition,
    },
};

use super::{
    output_selection::OutputSelection,
    rectangle_selection::{DragState, RectangleSelection},
    settings_drawer::build_settings_drawer,
    tool_button::build_shape_popup,
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
                let inside = if let Some((x, y, w, h)) = self.selection_rect {
                    pos.x >= x && pos.x <= x + w && pos.y >= y && pos.y <= y + h
                } else {
                    false
                };
                if !inside {
                    return (canvas::event::Status::Ignored, None);
                }

                // Convert to global coordinates
                let gx = pos.x + self.output_rect.left as f32;
                let gy = pos.y + self.output_rect.top as f32;

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
                let gx = pos.x + self.output_rect.left as f32;
                let gy = pos.y + self.output_rect.top as f32;

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

        let shape_color: Color = self.shape_color.into();
        let stroke = Stroke {
            style: shape_color.into(),
            width: 3.0,
            ..Stroke::default()
        };
        let shadow_stroke = Stroke {
            style: Color::from_rgba(0.0, 0.0, 0.0, 0.9).into(),
            width: 5.0,
            ..Stroke::default()
        };

        // Draw rectangle outlines
        for r in &self.rect_outlines {
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
            if self.shape_shadow {
                frame.stroke(&path, shadow_stroke);
            }
            frame.stroke(&path, stroke);
        }

        // Draw circle/ellipse outlines as a single path each (polyline), so joins/caps are handled by stroke.
        for c in &self.circles {
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
            if self.shape_shadow {
                frame.stroke(&path, shadow_stroke);
            }
            frame.stroke(&path, stroke);
        }

        // Live previews (during drag)
        let constrain = state.ctrl_down || state.ctrl_latched;
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
    /// Callback for setting shape color
    pub on_set_shape_color: Option<Box<dyn Fn(crate::config::ShapeColor) -> Msg + 'a>>,
    /// Callback for toggling shape shadow
    pub on_toggle_shape_shadow: Option<Msg>,
    /// Whether there are any annotations (for clear button in popup)
    pub has_any_annotations: bool,
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
            Choice::Output(ref selected_output) => {
                let is_selected = selected_output == &output.name;
                OutputSelection::new(on_output_change(output.output.clone()))
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
                if let Some(img) = toplevel_images
                    .get(win_output)
                    .and_then(|imgs| imgs.get(*win_idx))
                {
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
                Some((
                    0.0,
                    0.0,
                    output.logical_size.0 as f32,
                    output.logical_size.1 as f32,
                ))
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
                // Check if a selection is complete (can show action buttons)
                let has_selection = match choice {
                    Choice::Rectangle(r, _) => r.dimensions().is_some(),
                    Choice::Window(_, Some(_)) => true,
                    Choice::Output(_) => true,
                    _ => false,
                };

                // Determine if shape mode is active (any of the shape modes)
                let shape_mode_active = match primary_shape_tool {
                    ShapeTool::Arrow => arrow_mode,
                    ShapeTool::Circle => circle_mode,
                    ShapeTool::Rectangle => rect_outline_mode,
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
                    redact_mode,
                    pixelate_mode,
                    space_s,
                    space_xs,
                    space_xxs,
                    on_choice_change,
                    on_copy_to_clipboard,
                    on_save_to_pictures,
                    on_shape_popup_toggle.clone(),
                    on_open_shape_popup.clone(),
                    on_redact_toggle.clone(),
                    on_pixelate_toggle.clone(),
                    on_ocr.clone(),
                    on_ocr_copy.clone(),
                    on_qr.clone(),
                    on_qr_copy.clone(),
                    on_cancel,
                    &on_toolbar_position,
                    on_settings_toggle.clone(),
                    settings_drawer_open,
                    settings_drawer_open || shape_popup_open, // Keep toolbar opaque when either popup is open
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
            // Handle arrow drawing mode - press to start, release to end
            if self.arrow_mode {
                // Check if position is inside selection rectangle
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        pos.x >= sel_x
                            && pos.x <= sel_x + sel_w
                            && pos.y >= sel_y
                            && pos.y <= sel_y + sel_h
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

            // Handle redact drawing mode - press to start, release to end
            if self.redact_mode {
                // Check if position is inside selection rectangle
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        pos.x >= sel_x
                            && pos.x <= sel_x + sel_w
                            && pos.y >= sel_y
                            && pos.y <= sel_y + sel_h
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        // Start a new redaction on press
                        let global_x = pos.x + self.output_rect.left as f32;
                        let global_y = pos.y + self.output_rect.top as f32;
                        if let Some(ref on_redact_start) = self.on_redact_start {
                            shell.publish(on_redact_start(global_x, global_y));
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    MouseEvent::ButtonReleased(Button::Left) if self.redact_drawing.is_some() => {
                        // Finish the redaction on release
                        let global_x = pos.x + self.output_rect.left as f32;
                        let global_y = pos.y + self.output_rect.top as f32;
                        if let Some(ref on_redact_end) = self.on_redact_end {
                            shell.publish(on_redact_end(global_x, global_y));
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    _ => {}
                }
            }

            // Handle pixelate drawing mode - press to start, release to end
            if self.pixelate_mode {
                // Check if position is inside selection rectangle
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        pos.x >= sel_x
                            && pos.x <= sel_x + sel_w
                            && pos.y >= sel_y
                            && pos.y <= sel_y + sel_h
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        // Start a new pixelation on press
                        let global_x = pos.x + self.output_rect.left as f32;
                        let global_y = pos.y + self.output_rect.top as f32;
                        if let Some(ref on_pixelate_start) = self.on_pixelate_start {
                            shell.publish(on_pixelate_start(global_x, global_y));
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    MouseEvent::ButtonReleased(Button::Left) if self.pixelate_drawing.is_some() => {
                        // Finish the pixelation on release
                        let global_x = pos.x + self.output_rect.left as f32;
                        let global_y = pos.y + self.output_rect.top as f32;
                        if let Some(ref on_pixelate_end) = self.on_pixelate_end {
                            shell.publish(on_pixelate_end(global_x, global_y));
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

        // Draw fg_element (rectangle selection overlay)
        if let Some((i, (layout, child))) = children_iter.next() {
            renderer.with_layer(layout.bounds(), |renderer| {
                let tree = &tree.children[i];
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        }

        // Draw shapes canvas overlay (circle/rectangle outlines)
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

            // Shaft end (before arrowhead)
            let shaft_end_x = end_x - nx * head_size;
            let shaft_end_y = end_y - ny * head_size;

            // Feather widths in logical pixels to soften edges (simulates AA even without MSAA).
            // We use a 3-step alpha ramp (outer=0, mid≈0.35, inner=1) and also feather the caps
            // by extending the mid/outer layers along the arrow direction.
            let feather_outer = 3.0_f32;
            let feather_mid = 1.5_f32;

            // Perpendicular vectors for inner/mid/outer radii
            let r_in = thickness / 2.0;
            let r_mid = r_in + feather_mid;
            let r_out = r_in + feather_outer;

            let px_in = -ny * r_in;
            let py_in = nx * r_in;
            let px_mid = -ny * r_mid;
            let py_mid = nx * r_mid;
            let px_out = -ny * r_out;
            let py_out = nx * r_out;

            let mut inner = color;
            inner.a = inner.a.clamp(0.0, 1.0);
            let packed_inner = pack(inner);

            let mut mid = color;
            mid.a = (inner.a * 0.35).clamp(0.12, 0.45);
            let packed_mid = pack(mid);

            let mut outer = color;
            outer.a = 0.0;
            let packed_outer = pack(outer);

            // Cap feathering: extend mid/outer a bit along the line direction to soften end caps.
            let start_mid_x = start_x - nx * feather_mid;
            let start_mid_y = start_y - ny * feather_mid;
            let end_mid_x = shaft_end_x + nx * (feather_mid * 0.5);
            let end_mid_y = shaft_end_y + ny * (feather_mid * 0.5);

            let start_out_x = start_x - nx * feather_outer;
            let start_out_y = start_y - ny * feather_outer;
            let end_out_x = shaft_end_x + nx * (feather_outer * 0.5);
            let end_out_y = shaft_end_y + ny * (feather_outer * 0.5);

            // Vertex layout:
            // Shaft: outer(0..3), mid(4..7), inner(8..11)
            // Head:  outer(12..14), mid(15..17), inner(18..20)
            let mut vertices = Vec::with_capacity(21);

            // Shaft outer quad
            vertices.push(SolidVertex2D {
                position: [start_out_x + px_out, start_out_y + py_out],
                color: packed_outer,
            }); // 0
            vertices.push(SolidVertex2D {
                position: [start_out_x - px_out, start_out_y - py_out],
                color: packed_outer,
            }); // 1
            vertices.push(SolidVertex2D {
                position: [end_out_x - px_out, end_out_y - py_out],
                color: packed_outer,
            }); // 2
            vertices.push(SolidVertex2D {
                position: [end_out_x + px_out, end_out_y + py_out],
                color: packed_outer,
            }); // 3

            // Shaft mid quad
            vertices.push(SolidVertex2D {
                position: [start_mid_x + px_mid, start_mid_y + py_mid],
                color: packed_mid,
            }); // 4
            vertices.push(SolidVertex2D {
                position: [start_mid_x - px_mid, start_mid_y - py_mid],
                color: packed_mid,
            }); // 5
            vertices.push(SolidVertex2D {
                position: [end_mid_x - px_mid, end_mid_y - py_mid],
                color: packed_mid,
            }); // 6
            vertices.push(SolidVertex2D {
                position: [end_mid_x + px_mid, end_mid_y + py_mid],
                color: packed_mid,
            }); // 7

            // Shaft inner quad (no cap extension; keep geometry accurate)
            vertices.push(SolidVertex2D {
                position: [start_x + px_in, start_y + py_in],
                color: packed_inner,
            }); // 8
            vertices.push(SolidVertex2D {
                position: [start_x - px_in, start_y - py_in],
                color: packed_inner,
            }); // 9
            vertices.push(SolidVertex2D {
                position: [shaft_end_x - px_in, shaft_end_y - py_in],
                color: packed_inner,
            }); // 10
            vertices.push(SolidVertex2D {
                position: [shaft_end_x + px_in, shaft_end_y + py_in],
                color: packed_inner,
            }); // 11

            // Arrowhead (wider than shaft)
            let head_width = head_size * 0.5;
            let h_in = head_width;
            let h_mid = head_width + feather_mid;
            let h_out = head_width + feather_outer;

            let hpx_in = -ny * h_in;
            let hpy_in = nx * h_in;
            let hpx_mid = -ny * h_mid;
            let hpy_mid = nx * h_mid;
            let hpx_out = -ny * h_out;
            let hpy_out = nx * h_out;

            // Outer head triangle (alpha 0), extended forward
            vertices.push(SolidVertex2D {
                position: [shaft_end_x + hpx_out, shaft_end_y + hpy_out],
                color: packed_outer,
            }); // 12
            vertices.push(SolidVertex2D {
                position: [shaft_end_x - hpx_out, shaft_end_y - hpy_out],
                color: packed_outer,
            }); // 13
            vertices.push(SolidVertex2D {
                position: [end_x + nx * feather_outer, end_y + ny * feather_outer],
                color: packed_outer,
            }); // 14

            // Mid head triangle (alpha ~0.35), extended forward
            vertices.push(SolidVertex2D {
                position: [shaft_end_x + hpx_mid, shaft_end_y + hpy_mid],
                color: packed_mid,
            }); // 15
            vertices.push(SolidVertex2D {
                position: [shaft_end_x - hpx_mid, shaft_end_y - hpy_mid],
                color: packed_mid,
            }); // 16
            vertices.push(SolidVertex2D {
                position: [end_x + nx * feather_mid, end_y + ny * feather_mid],
                color: packed_mid,
            }); // 17

            // Inner head triangle (alpha 1)
            vertices.push(SolidVertex2D {
                position: [shaft_end_x + hpx_in, shaft_end_y + hpy_in],
                color: packed_inner,
            }); // 18
            vertices.push(SolidVertex2D {
                position: [shaft_end_x - hpx_in, shaft_end_y - hpy_in],
                color: packed_inner,
            }); // 19
            vertices.push(SolidVertex2D {
                position: [end_x, end_y],
                color: packed_inner,
            }); // 20

            // Indices:
            // - Inner shaft quad (solid)
            // - Mid band around shaft (outer<->mid and mid<->inner, both sides)
            // - Inner head triangle (solid)
            // - Mid/outer bands around head (base edge + 2 side edges)
            let indices: Vec<u32> = vec![
                // Inner shaft
                8, 9, 10, 8, 10, 11,
                // Shaft band: mid <-> inner (+ side)
                4, 8, 11, 4, 11, 7,
                // Shaft band: mid <-> inner (- side)
                5, 6, 10, 5, 10, 9,
                // Shaft band: outer <-> mid (+ side)
                0, 4, 7, 0, 7, 3,
                // Shaft band: outer <-> mid (- side)
                1, 2, 6, 1, 6, 5,
                // Inner head
                18, 19, 20,
                // Head band: mid <-> inner base edge
                15, 18, 19, 15, 19, 16,
                // Head band: mid <-> inner + edge to tip
                15, 18, 20, 15, 20, 17,
                // Head band: mid <-> inner - edge to tip
                16, 19, 20, 16, 20, 17,
                // Head band: outer <-> mid base edge
                12, 15, 16, 12, 16, 13,
                // Head band: outer <-> mid + edge to tip
                12, 15, 17, 12, 17, 14,
                // Head band: outer <-> mid - edge to tip
                13, 16, 17, 13, 17, 14,
            ];

            Some((vertices, indices))
        }

        // Draw redactions (black rectangles)
        let redact_color = cosmic::iced::Color::BLACK;

        for redact in &self.redactions {
            // Convert global coordinates to widget-local
            let x1 = redact.x - self.output_rect.left as f32;
            let y1 = redact.y - self.output_rect.top as f32;
            let x2 = redact.x2 - self.output_rect.left as f32;
            let y2 = redact.y2 - self.output_rect.top as f32;

            // Normalize (ensure min < max)
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

        // Draw pixelation previews (mosaic pattern to indicate pixelated areas)
        for pixelate in &self.pixelations {
            // Convert global coordinates to widget-local
            let x1 = pixelate.x - self.output_rect.left as f32;
            let y1 = pixelate.y - self.output_rect.top as f32;
            let x2 = pixelate.x2 - self.output_rect.left as f32;
            let y2 = pixelate.y2 - self.output_rect.top as f32;

            // Normalize (ensure min < max)
            let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
            let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

            // Draw a checkerboard pattern to indicate pixelation
            let block_size = 16.0_f32;
            let color1 = cosmic::iced::Color::from_rgba(0.3, 0.3, 0.3, 0.7);
            let color2 = cosmic::iced::Color::from_rgba(0.6, 0.6, 0.6, 0.7);

            renderer.with_layer(*viewport, |renderer| {
                let mut row = 0;
                let mut y = min_y;
                while y < max_y {
                    let mut col = 0;
                    let mut x = min_x;
                    let block_h = block_size.min(max_y - y);
                    while x < max_x {
                        let block_w = block_size.min(max_x - x);
                        let color = if (row + col) % 2 == 0 { color1 } else { color2 };
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
                        x += block_size;
                        col += 1;
                    }
                    y += block_size;
                    row += 1;
                }
            });
        }

        // Draw redaction preview (currently being drawn)
        if let Some((start_x, start_y)) = self.redact_drawing
            && let Some(cursor_pos) = cursor.position()
        {
            let x1 = start_x - self.output_rect.left as f32;
            let y1 = start_y - self.output_rect.top as f32;
            let x2 = cursor_pos.x;
            let y2 = cursor_pos.y;

            let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
            let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

            let rect = cosmic::iced_core::Rectangle {
                x: min_x,
                y: min_y,
                width: max_x - min_x,
                height: max_y - min_y,
            };

            let preview_color = cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.7);

            renderer.with_layer(*viewport, |renderer| {
                renderer.fill_quad(
                    cosmic::iced_core::renderer::Quad {
                        bounds: rect,
                        border: Border {
                            radius: 0.0.into(),
                            width: 2.0,
                            color: cosmic::iced::Color::WHITE,
                        },
                        shadow: cosmic::iced_core::Shadow::default(),
                    },
                    Background::Color(preview_color),
                );
            });
        }

        // Draw pixelation preview (currently being drawn) - use a checkered pattern
        if let Some((start_x, start_y)) = self.pixelate_drawing
            && let Some(cursor_pos) = cursor.position()
        {
            let x1 = start_x - self.output_rect.left as f32;
            let y1 = start_y - self.output_rect.top as f32;
            let x2 = cursor_pos.x;
            let y2 = cursor_pos.y;

            let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
            let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

            let rect = cosmic::iced_core::Rectangle {
                x: min_x,
                y: min_y,
                width: max_x - min_x,
                height: max_y - min_y,
            };

            // Use a semi-transparent gray with a mosaic-like border
            let preview_color = cosmic::iced::Color::from_rgba(0.5, 0.5, 0.5, 0.5);

            renderer.with_layer(*viewport, |renderer| {
                renderer.fill_quad(
                    cosmic::iced_core::renderer::Quad {
                        bounds: rect,
                        border: Border {
                            radius: 0.0.into(),
                            width: 2.0,
                            color: cosmic::iced::Color::from_rgba(0.8, 0.8, 0.8, 1.0),
                        },
                        shadow: cosmic::iced_core::Shadow::default(),
                    },
                    Background::Color(preview_color),
                );
            });
        }

        // Draw arrows on top of the selection using meshes
        let shape_color: cosmic::iced::Color = self.shape_color.into();
        let border_color = cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.9);
        let arrow_thickness = 4.0_f32;
        let head_size = 16.0_f32;
        let outline_px = 1.0_f32;

        for arrow in &self.arrows {
            // Convert global coordinates to widget-local
            let start_x = arrow.start_x - self.output_rect.left as f32;
            let start_y = arrow.start_y - self.output_rect.top as f32;
            let end_x = arrow.end_x - self.output_rect.left as f32;
            let end_y = arrow.end_y - self.output_rect.top as f32;

            // Border/shadow first, then main arrow
            if self.shape_shadow {
                if let Some((vertices, indices)) = build_arrow_mesh(
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

            if let Some((vertices, indices)) = build_arrow_mesh(
                start_x,
                start_y,
                end_x,
                end_y,
                shape_color,
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

        // Draw arrow currently being drawn (preview)
        if let Some((start_x, start_y)) = self.arrow_drawing
            && let Some(cursor_pos) = cursor.position()
        {
            let local_start_x = start_x - self.output_rect.left as f32;
            let local_start_y = start_y - self.output_rect.top as f32;
            let end_x = cursor_pos.x;
            let end_y = cursor_pos.y;

            let mut preview_color = shape_color;
            preview_color.a = 0.7;
            let preview_border_color = cosmic::iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6);

            if self.shape_shadow {
                if let Some((vertices, indices)) = build_arrow_mesh(
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

            if let Some((vertices, indices)) = build_arrow_mesh(
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

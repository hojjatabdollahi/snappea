//! AnnotationCanvas widget for rendering and handling annotation drawing
//!
//! This widget handles:
//! - Rendering completed annotations (arrows, circles, rectangles, redactions, pixelations)
//! - Rendering in-progress annotation previews
//! - Mouse events for starting/updating/completing annotations

use cosmic::{
    Element,
    iced::{Color, mouse},
    iced_core::{
        Background, Border, Clipboard, Layout, Length, Rectangle, Shell, Size, Widget, event,
        layout, overlay,
        renderer::Renderer as RendererTrait,
        widget::{Tree, tree},
    },
    iced_widget::graphics::{
        Mesh,
        mesh::{Indexed, Renderer as MeshRenderer},
    },
};
use image::RgbaImage;

use crate::{
    config::ShapeColor,
    domain::{
        ArrowAnnotation, CircleOutlineAnnotation, PixelateAnnotation, Rect, RectOutlineAnnotation,
        RedactAnnotation,
    },
    render::mesh::build_arrow_mesh,
};

/// Current drawing mode for annotations
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingMode {
    None,
    Arrow,
    Circle,
    Rectangle,
    Redact,
    Pixelate,
}

/// Messages emitted by the AnnotationCanvas
#[derive(Clone, Debug)]
pub enum AnnotationEvent {
    /// Arrow drawing started at position
    ArrowStart(f32, f32),
    /// Arrow drawing ended at position
    ArrowEnd(f32, f32),
    /// Circle drawing started at position
    CircleStart(f32, f32),
    /// Circle drawing ended at position
    CircleEnd(f32, f32),
    /// Rectangle drawing started at position
    RectangleStart(f32, f32),
    /// Rectangle drawing ended at position
    RectangleEnd(f32, f32),
    /// Redact drawing started at position
    RedactStart(f32, f32),
    /// Redact drawing ended at position
    RedactEnd(f32, f32),
    /// Pixelate drawing started at position
    PixelateStart(f32, f32),
    /// Pixelate drawing ended at position
    PixelateEnd(f32, f32),
}

/// Configuration for the annotation canvas
pub struct AnnotationCanvasConfig<'a> {
    /// Output rect offset for coordinate conversion
    pub output_rect: Rect,
    /// Selection rect in local coordinates (x, y, w, h) - annotations are clamped to this
    pub selection_rect: Option<(f32, f32, f32, f32)>,
    /// Current drawing mode
    pub mode: DrawingMode,
    /// Current drawing start point (if any)
    pub drawing_start: Option<(f32, f32)>,
    /// Shape color for new annotations
    pub shape_color: ShapeColor,
    /// Whether shadow is enabled for shapes
    pub shape_shadow: bool,
    /// Pixelation block size
    pub pixelation_block_size: u32,
    /// Image scale factor
    pub image_scale: f32,
    /// Reference to screenshot image (for pixelation preview)
    pub screenshot_image: &'a RgbaImage,
    /// Optional window image (for window mode pixelation)
    pub window_image: Option<&'a RgbaImage>,
    /// Window display info: (x, y, width, height, display_to_image_scale)
    pub window_display_info: Option<(f32, f32, f32, f32, f32)>,
}

/// AnnotationCanvas widget
pub struct AnnotationCanvas<'a, Msg> {
    /// Completed arrow annotations
    arrows: &'a [ArrowAnnotation],
    /// Completed circle annotations
    circles: &'a [CircleOutlineAnnotation],
    /// Completed rectangle annotations
    rect_outlines: &'a [RectOutlineAnnotation],
    /// Completed redaction annotations
    redactions: &'a [RedactAnnotation],
    /// Completed pixelation annotations
    pixelations: &'a [PixelateAnnotation],
    /// Configuration
    config: AnnotationCanvasConfig<'a>,
    /// Event handler
    on_event: Option<Box<dyn Fn(AnnotationEvent) -> Msg + 'a>>,
}

impl<'a, Msg> AnnotationCanvas<'a, Msg> {
    /// Create a new annotation canvas
    pub fn new(config: AnnotationCanvasConfig<'a>) -> Self {
        Self {
            arrows: &[],
            circles: &[],
            rect_outlines: &[],
            redactions: &[],
            pixelations: &[],
            config,
            on_event: None,
        }
    }

    /// Set arrow annotations
    pub fn arrows(mut self, arrows: &'a [ArrowAnnotation]) -> Self {
        self.arrows = arrows;
        self
    }

    /// Set circle annotations
    pub fn circles(mut self, circles: &'a [CircleOutlineAnnotation]) -> Self {
        self.circles = circles;
        self
    }

    /// Set rectangle outline annotations
    pub fn rect_outlines(mut self, rect_outlines: &'a [RectOutlineAnnotation]) -> Self {
        self.rect_outlines = rect_outlines;
        self
    }

    /// Set redaction annotations
    pub fn redactions(mut self, redactions: &'a [RedactAnnotation]) -> Self {
        self.redactions = redactions;
        self
    }

    /// Set pixelation annotations
    pub fn pixelations(mut self, pixelations: &'a [PixelateAnnotation]) -> Self {
        self.pixelations = pixelations;
        self
    }

    /// Set event handler
    pub fn on_event(mut self, handler: impl Fn(AnnotationEvent) -> Msg + 'a) -> Self {
        self.on_event = Some(Box::new(handler));
        self
    }
}

impl<'a, Msg: Clone + 'static> Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for AnnotationCanvas<'a, Msg>
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<()>()
    }

    fn state(&self) -> tree::State {
        tree::State::None
    }

    fn children(&self) -> Vec<Tree> {
        vec![]
    }

    fn diff(&mut self, _tree: &mut Tree) {}

    fn layout(
        &self,
        _tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut cosmic::Renderer,
        _theme: &cosmic::Theme,
        _style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        // Draw completed redactions (filled black rectangles)
        for redact in self.redactions {
            let x1 = redact.x - self.config.output_rect.left as f32;
            let y1 = redact.y - self.config.output_rect.top as f32;
            let x2 = redact.x2 - self.config.output_rect.left as f32;
            let y2 = redact.y2 - self.config.output_rect.top as f32;
            let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
            let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: Rectangle {
                        x: min_x,
                        y: min_y,
                        width: max_x - min_x,
                        height: max_y - min_y,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                Background::Color(Color::BLACK),
            );
        }

        // Draw completed pixelations
        self.draw_pixelations(renderer, &bounds);

        // Draw completed shape annotations (circles, rectangles)
        self.draw_circles(renderer, &bounds);
        self.draw_rectangles(renderer, &bounds);

        // Draw completed arrows
        self.draw_arrows(renderer, &bounds);

        // Draw preview based on current mode
        if let Some(cursor_pos) = cursor.position()
            && let Some((start_x, start_y)) = self.config.drawing_start
        {
            let local_start_x = start_x - self.config.output_rect.left as f32;
            let local_start_y = start_y - self.config.output_rect.top as f32;
            let end_x = cursor_pos.x;
            let end_y = cursor_pos.y;

            match self.config.mode {
                DrawingMode::Arrow => {
                    self.draw_arrow_preview(
                        renderer,
                        &bounds,
                        local_start_x,
                        local_start_y,
                        end_x,
                        end_y,
                    );
                }
                DrawingMode::Circle => {
                    self.draw_circle_preview(renderer, local_start_x, local_start_y, end_x, end_y);
                }
                DrawingMode::Rectangle => {
                    self.draw_rectangle_preview(
                        renderer,
                        local_start_x,
                        local_start_y,
                        end_x,
                        end_y,
                    );
                }
                DrawingMode::Redact => {
                    self.draw_redact_preview(renderer, local_start_x, local_start_y, end_x, end_y);
                }
                DrawingMode::Pixelate => {
                    self.draw_pixelate_preview(
                        renderer,
                        local_start_x,
                        local_start_y,
                        end_x,
                        end_y,
                    );
                }
                DrawingMode::None => {}
            }
        }
    }

    fn on_event(
        &mut self,
        _tree: &mut Tree,
        event: cosmic::iced_core::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &cosmic::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Msg>,
        _viewport: &Rectangle,
    ) -> event::Status {
        let Some(on_event) = &self.on_event else {
            return event::Status::Ignored;
        };

        let bounds = layout.bounds();

        // Helper to clamp position within selection rect
        let clamp_to_selection = |x: f32, y: f32| -> (f32, f32) {
            if let Some((sel_x, sel_y, sel_w, sel_h)) = self.config.selection_rect {
                let clamped_x = x.max(sel_x).min(sel_x + sel_w);
                let clamped_y = y.max(sel_y).min(sel_y + sel_h);
                (clamped_x, clamped_y)
            } else {
                (x, y)
            }
        };

        // Check if position is inside selection rect
        let is_inside_selection = |x: f32, y: f32| -> bool {
            if let Some((sel_x, sel_y, sel_w, sel_h)) = self.config.selection_rect {
                x >= sel_x && x <= sel_x + sel_w && y >= sel_y && y <= sel_y + sel_h
            } else {
                true // No selection means entire area is valid
            }
        };

        match event {
            cosmic::iced_core::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    // Only start drawing if inside selection
                    if !is_inside_selection(pos.x, pos.y) {
                        return event::Status::Ignored;
                    }

                    let (clamped_x, clamped_y) = clamp_to_selection(pos.x, pos.y);
                    let global_x = clamped_x + self.config.output_rect.left as f32;
                    let global_y = clamped_y + self.config.output_rect.top as f32;

                    let msg = match self.config.mode {
                        DrawingMode::Arrow => {
                            Some(on_event(AnnotationEvent::ArrowStart(global_x, global_y)))
                        }
                        DrawingMode::Circle => {
                            Some(on_event(AnnotationEvent::CircleStart(global_x, global_y)))
                        }
                        DrawingMode::Rectangle => Some(on_event(AnnotationEvent::RectangleStart(
                            global_x, global_y,
                        ))),
                        DrawingMode::Redact => {
                            Some(on_event(AnnotationEvent::RedactStart(global_x, global_y)))
                        }
                        DrawingMode::Pixelate => {
                            Some(on_event(AnnotationEvent::PixelateStart(global_x, global_y)))
                        }
                        DrawingMode::None => None,
                    };

                    if let Some(msg) = msg {
                        shell.publish(msg);
                        return event::Status::Captured;
                    }
                }
            }
            cosmic::iced_core::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if self.config.drawing_start.is_some()
                    && let Some(pos) = cursor.position()
                {
                    // Clamp to selection and convert to global
                    let (clamped_x, clamped_y) = clamp_to_selection(pos.x, pos.y);
                    let global_x = clamped_x + self.config.output_rect.left as f32;
                    let global_y = clamped_y + self.config.output_rect.top as f32;

                    let msg = match self.config.mode {
                        DrawingMode::Arrow => {
                            Some(on_event(AnnotationEvent::ArrowEnd(global_x, global_y)))
                        }
                        DrawingMode::Circle => {
                            Some(on_event(AnnotationEvent::CircleEnd(global_x, global_y)))
                        }
                        DrawingMode::Rectangle => {
                            Some(on_event(AnnotationEvent::RectangleEnd(global_x, global_y)))
                        }
                        DrawingMode::Redact => {
                            Some(on_event(AnnotationEvent::RedactEnd(global_x, global_y)))
                        }
                        DrawingMode::Pixelate => {
                            Some(on_event(AnnotationEvent::PixelateEnd(global_x, global_y)))
                        }
                        DrawingMode::None => None,
                    };

                    if let Some(msg) = msg {
                        shell.publish(msg);
                        return event::Status::Captured;
                    }
                }
            }
            _ => {}
        }

        event::Status::Ignored
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        if self.config.mode != DrawingMode::None && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut Tree,
        _layout: Layout<'_>,
        _renderer: &cosmic::Renderer,
        _translation: cosmic::iced::Vector,
    ) -> Option<overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>> {
        None
    }
}

impl<'a, Msg: Clone + 'static> AnnotationCanvas<'a, Msg> {
    fn draw_arrows(&self, renderer: &mut cosmic::Renderer, viewport: &Rectangle) {
        let border_color = Color::from_rgba(0.0, 0.0, 0.0, 0.9);
        let arrow_thickness = 4.0_f32;
        let head_size = 16.0_f32;
        let outline_px = 1.0_f32;

        for arrow in self.arrows {
            let arrow_color: Color = arrow.color.into();
            let start_x = arrow.start_x - self.config.output_rect.left as f32;
            let start_y = arrow.start_y - self.config.output_rect.top as f32;
            let end_x = arrow.end_x - self.config.output_rect.left as f32;
            let end_y = arrow.end_y - self.config.output_rect.top as f32;

            // Draw shadow first
            if arrow.shadow
                && let Some((vertices, indices)) = build_arrow_mesh(
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    border_color,
                    arrow_thickness + 2.0 * outline_px,
                    head_size + outline_px,
                )
            {
                renderer.with_layer(*viewport, |renderer| {
                    renderer.draw_mesh(Mesh::Solid {
                        buffers: Indexed { vertices, indices },
                        transformation: cosmic::iced_core::Transformation::IDENTITY,
                        clip_bounds: *viewport,
                    });
                });
            }

            // Draw main arrow
            if let Some((vertices, indices)) = build_arrow_mesh(
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
    }

    fn draw_arrow_preview(
        &self,
        renderer: &mut cosmic::Renderer,
        viewport: &Rectangle,
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
    ) {
        let arrow_thickness = 4.0_f32;
        let head_size = 16.0_f32;
        let outline_px = 1.0_f32;

        let mut preview_color: Color = self.config.shape_color.into();
        preview_color.a = 0.7;
        let preview_border_color = Color::from_rgba(0.0, 0.0, 0.0, 0.6);

        // Draw shadow
        if self.config.shape_shadow
            && let Some((vertices, indices)) = build_arrow_mesh(
                start_x,
                start_y,
                end_x,
                end_y,
                preview_border_color,
                arrow_thickness + 2.0 * outline_px,
                head_size + outline_px,
            )
        {
            renderer.with_layer(*viewport, |renderer| {
                renderer.draw_mesh(Mesh::Solid {
                    buffers: Indexed { vertices, indices },
                    transformation: cosmic::iced_core::Transformation::IDENTITY,
                    clip_bounds: *viewport,
                });
            });
        }

        // Draw preview arrow
        if let Some((vertices, indices)) = build_arrow_mesh(
            start_x,
            start_y,
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

    fn draw_circles(&self, renderer: &mut cosmic::Renderer, _viewport: &Rectangle) {
        let stroke_width = 3.0_f32;
        let outline_px = 1.0_f32;

        for circle in self.circles {
            let circle_color: Color = circle.color.into();
            // Convert from start/end to center/radius
            let start_x = circle.start_x - self.config.output_rect.left as f32;
            let start_y = circle.start_y - self.config.output_rect.top as f32;
            let end_x = circle.end_x - self.config.output_rect.left as f32;
            let end_y = circle.end_y - self.config.output_rect.top as f32;
            let cx = (start_x + end_x) / 2.0;
            let cy = (start_y + end_y) / 2.0;
            let radius = ((end_x - start_x).powi(2) + (end_y - start_y).powi(2)).sqrt() / 2.0;

            // Draw shadow
            if circle.shadow {
                self.draw_circle_stroke(
                    renderer,
                    cx,
                    cy,
                    radius,
                    Color::from_rgba(0.0, 0.0, 0.0, 0.9),
                    stroke_width + 2.0 * outline_px,
                );
            }

            // Draw circle
            self.draw_circle_stroke(renderer, cx, cy, radius, circle_color, stroke_width);
        }
    }

    fn draw_circle_preview(
        &self,
        renderer: &mut cosmic::Renderer,
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
    ) {
        let cx = (start_x + end_x) / 2.0;
        let cy = (start_y + end_y) / 2.0;
        let radius = ((end_x - start_x).powi(2) + (end_y - start_y).powi(2)).sqrt() / 2.0;

        let stroke_width = 3.0_f32;
        let outline_px = 1.0_f32;

        let mut preview_color: Color = self.config.shape_color.into();
        preview_color.a = 0.7;

        // Draw shadow
        if self.config.shape_shadow {
            self.draw_circle_stroke(
                renderer,
                cx,
                cy,
                radius,
                Color::from_rgba(0.0, 0.0, 0.0, 0.6),
                stroke_width + 2.0 * outline_px,
            );
        }

        // Draw preview
        self.draw_circle_stroke(renderer, cx, cy, radius, preview_color, stroke_width);
    }

    fn draw_circle_stroke(
        &self,
        renderer: &mut cosmic::Renderer,
        cx: f32,
        cy: f32,
        radius: f32,
        color: Color,
        stroke_width: f32,
    ) {
        // Approximate circle with line segments
        let segments = 64;
        let half_stroke = stroke_width / 2.0;

        for i in 0..segments {
            let angle1 = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let angle2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;

            let x1 = cx + radius * angle1.cos();
            let y1 = cy + radius * angle1.sin();
            let x2 = cx + radius * angle2.cos();
            let y2 = cy + radius * angle2.sin();

            // Draw small quad for each segment
            let dx = x2 - x1;
            let dy = y2 - y1;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 0.001 {
                continue;
            }
            let nx = -dy / len * half_stroke;
            let ny = dx / len * half_stroke;

            let min_x = (x1 + nx).min(x1 - nx).min(x2 + nx).min(x2 - nx);
            let min_y = (y1 + ny).min(y1 - ny).min(y2 + ny).min(y2 - ny);
            let max_x = (x1 + nx).max(x1 - nx).max(x2 + nx).max(x2 - nx);
            let max_y = (y1 + ny).max(y1 - ny).max(y2 + ny).max(y2 - ny);

            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: Rectangle {
                        x: min_x,
                        y: min_y,
                        width: max_x - min_x,
                        height: max_y - min_y,
                    },
                    border: Border::default(),
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                Background::Color(color),
            );
        }
    }

    fn draw_rectangles(&self, renderer: &mut cosmic::Renderer, _viewport: &Rectangle) {
        let stroke_width = 3.0_f32;
        let outline_px = 1.0_f32;

        for rect in self.rect_outlines {
            let rect_color: Color = rect.color.into();
            let x1 = rect.start_x - self.config.output_rect.left as f32;
            let y1 = rect.start_y - self.config.output_rect.top as f32;
            let x2 = rect.end_x - self.config.output_rect.left as f32;
            let y2 = rect.end_y - self.config.output_rect.top as f32;
            let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
            let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

            // Draw shadow
            if rect.shadow {
                self.draw_rect_stroke(
                    renderer,
                    min_x,
                    min_y,
                    max_x - min_x,
                    max_y - min_y,
                    Color::from_rgba(0.0, 0.0, 0.0, 0.9),
                    stroke_width + 2.0 * outline_px,
                );
            }

            // Draw rectangle
            self.draw_rect_stroke(
                renderer,
                min_x,
                min_y,
                max_x - min_x,
                max_y - min_y,
                rect_color,
                stroke_width,
            );
        }
    }

    fn draw_rectangle_preview(
        &self,
        renderer: &mut cosmic::Renderer,
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
    ) {
        let (min_x, max_x) = if start_x < end_x {
            (start_x, end_x)
        } else {
            (end_x, start_x)
        };
        let (min_y, max_y) = if start_y < end_y {
            (start_y, end_y)
        } else {
            (end_y, start_y)
        };

        let stroke_width = 3.0_f32;
        let outline_px = 1.0_f32;

        let mut preview_color: Color = self.config.shape_color.into();
        preview_color.a = 0.7;

        // Draw shadow
        if self.config.shape_shadow {
            self.draw_rect_stroke(
                renderer,
                min_x,
                min_y,
                max_x - min_x,
                max_y - min_y,
                Color::from_rgba(0.0, 0.0, 0.0, 0.6),
                stroke_width + 2.0 * outline_px,
            );
        }

        // Draw preview
        self.draw_rect_stroke(
            renderer,
            min_x,
            min_y,
            max_x - min_x,
            max_y - min_y,
            preview_color,
            stroke_width,
        );
    }

    fn draw_rect_stroke(
        &self,
        renderer: &mut cosmic::Renderer,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        stroke_width: f32,
    ) {
        let half_stroke = stroke_width / 2.0;

        // Top edge
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: Rectangle {
                    x: x - half_stroke,
                    y: y - half_stroke,
                    width: width + stroke_width,
                    height: stroke_width,
                },
                border: Border::default(),
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(color),
        );

        // Bottom edge
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: Rectangle {
                    x: x - half_stroke,
                    y: y + height - half_stroke,
                    width: width + stroke_width,
                    height: stroke_width,
                },
                border: Border::default(),
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(color),
        );

        // Left edge
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: Rectangle {
                    x: x - half_stroke,
                    y: y + half_stroke,
                    width: stroke_width,
                    height: height - stroke_width,
                },
                border: Border::default(),
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(color),
        );

        // Right edge
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: Rectangle {
                    x: x + width - half_stroke,
                    y: y + half_stroke,
                    width: stroke_width,
                    height: height - stroke_width,
                },
                border: Border::default(),
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(color),
        );
    }

    fn draw_redact_preview(
        &self,
        renderer: &mut cosmic::Renderer,
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
    ) {
        let (min_x, max_x) = if start_x < end_x {
            (start_x, end_x)
        } else {
            (end_x, start_x)
        };
        let (min_y, max_y) = if start_y < end_y {
            (start_y, end_y)
        } else {
            (end_y, start_y)
        };

        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: Rectangle {
                    x: min_x,
                    y: min_y,
                    width: max_x - min_x,
                    height: max_y - min_y,
                },
                border: Border {
                    color: Color::WHITE,
                    width: 1.0,
                    radius: cosmic::iced_core::border::Radius::from(0.0),
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.7)),
        );
    }

    fn draw_pixelations(&self, renderer: &mut cosmic::Renderer, viewport: &Rectangle) {
        for pixelate in self.pixelations {
            let x1 = pixelate.x - self.config.output_rect.left as f32;
            let y1 = pixelate.y - self.config.output_rect.top as f32;
            let x2 = pixelate.x2 - self.config.output_rect.left as f32;
            let y2 = pixelate.y2 - self.config.output_rect.top as f32;
            let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
            let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

            self.draw_pixelation_region(
                renderer,
                viewport,
                min_x,
                min_y,
                max_x,
                max_y,
                pixelate.block_size,
            );
        }
    }

    fn draw_pixelate_preview(
        &self,
        renderer: &mut cosmic::Renderer,
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
    ) {
        let (min_x, max_x) = if start_x < end_x {
            (start_x, end_x)
        } else {
            (end_x, start_x)
        };
        let (min_y, max_y) = if start_y < end_y {
            (start_y, end_y)
        } else {
            (end_y, start_y)
        };

        let viewport = Rectangle {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        };

        self.draw_pixelation_region(
            renderer,
            &viewport,
            min_x,
            min_y,
            max_x,
            max_y,
            self.config.pixelation_block_size,
        );

        // Draw border
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: viewport,
                border: Border {
                    color: Color::WHITE,
                    width: 1.0,
                    radius: cosmic::iced_core::border::Radius::from(0.0),
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(Color::TRANSPARENT),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_pixelation_region(
        &self,
        renderer: &mut cosmic::Renderer,
        _viewport: &Rectangle,
        min_x: f32,
        min_y: f32,
        max_x: f32,
        max_y: f32,
        block_size: u32,
    ) {
        // Check if we're in window mode
        if let (Some(win_img), Some((win_x, win_y, _, _, display_to_img_scale))) =
            (self.config.window_image, self.config.window_display_info)
        {
            let block_size_display = block_size as f32;
            let mut y = min_y;
            while y < max_y {
                let mut x = min_x;
                let block_h = block_size_display.min(max_y - y);
                while x < max_x {
                    let block_w = block_size_display.min(max_x - x);
                    let win_rel_x = x - win_x;
                    let win_rel_y = y - win_y;
                    let img_x = (win_rel_x * display_to_img_scale).round() as i32;
                    let img_y = (win_rel_y * display_to_img_scale).round() as i32;
                    let img_x2 = ((win_rel_x + block_w) * display_to_img_scale).round() as i32;
                    let img_y2 = ((win_rel_y + block_h) * display_to_img_scale).round() as i32;

                    if img_x >= 0
                        && img_y >= 0
                        && img_x2 > 0
                        && img_y2 > 0
                        && let Some(color) = Self::average_color(
                            win_img,
                            img_x as u32,
                            img_y as u32,
                            img_x2 as u32,
                            img_y2 as u32,
                        )
                    {
                        renderer.fill_quad(
                            cosmic::iced_core::renderer::Quad {
                                bounds: Rectangle {
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
        } else {
            // Regular mode
            let block_size_logical = block_size as f32 / self.config.image_scale;
            let mut y = min_y;
            while y < max_y {
                let mut x = min_x;
                let block_h = block_size_logical.min(max_y - y);
                while x < max_x {
                    let block_w = block_size_logical.min(max_x - x);
                    let img_x = (x * self.config.image_scale).round() as u32;
                    let img_y = (y * self.config.image_scale).round() as u32;
                    let img_x2 = ((x + block_w) * self.config.image_scale).round() as u32;
                    let img_y2 = ((y + block_h) * self.config.image_scale).round() as u32;

                    if let Some(color) = Self::average_color(
                        self.config.screenshot_image,
                        img_x,
                        img_y,
                        img_x2,
                        img_y2,
                    ) {
                        renderer.fill_quad(
                            cosmic::iced_core::renderer::Quad {
                                bounds: Rectangle {
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
        }
    }

    fn average_color(img: &RgbaImage, x1: u32, y1: u32, x2: u32, y2: u32) -> Option<Color> {
        let x1 = x1.min(img.width().saturating_sub(1));
        let y1 = y1.min(img.height().saturating_sub(1));
        let x2 = x2.min(img.width());
        let y2 = y2.min(img.height());

        if x2 <= x1 || y2 <= y1 {
            return None;
        }

        let mut total_r: u64 = 0;
        let mut total_g: u64 = 0;
        let mut total_b: u64 = 0;
        let mut pixel_count: u64 = 0;

        for py in y1..y2 {
            for px in x1..x2 {
                let pixel = img.get_pixel(px, py);
                total_r += pixel[0] as u64;
                total_g += pixel[1] as u64;
                total_b += pixel[2] as u64;
                pixel_count += 1;
            }
        }

        if pixel_count > 0 {
            Some(Color::from_rgb8(
                (total_r / pixel_count) as u8,
                (total_g / pixel_count) as u8,
                (total_b / pixel_count) as u8,
            ))
        } else {
            None
        }
    }
}

impl<'a, Msg: Clone + 'static> From<AnnotationCanvas<'a, Msg>> for Element<'a, Msg> {
    fn from(canvas: AnnotationCanvas<'a, Msg>) -> Self {
        Self::new(canvas)
    }
}

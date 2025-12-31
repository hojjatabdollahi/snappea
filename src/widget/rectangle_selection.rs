use std::borrow::Cow;

use cosmic::{
    iced::{
        clipboard::{
            dnd::{self, DndAction, DndDestinationRectangle, DndEvent, OfferEvent, SourceEvent},
            mime::{AllowedMimeTypes, AsMimeTypes},
        },
        mouse,
    },
    iced_core::{
        self, Border, Color, Length, Point, Rectangle, Shadow, Size,
        clipboard::DndSource, layout::Node, renderer::Quad,
    },
    iced_core::Renderer,
    iced_widget::graphics::mesh::Renderer as MeshRenderer,
    widget::{self, Widget},
};

use crate::screenshot::Rect;

pub const MIME: &str = "X-BLAZINGSHOT-MyData";
pub struct MyData;

impl From<(Vec<u8>, String)> for MyData {
    fn from(_: (Vec<u8>, String)) -> Self {
        MyData
    }
}

impl AllowedMimeTypes for MyData {
    fn allowed() -> std::borrow::Cow<'static, [String]> {
        std::borrow::Cow::Owned(vec![MIME.to_string()])
    }
}

impl AsMimeTypes for MyData {
    fn available(&self) -> std::borrow::Cow<'static, [String]> {
        std::borrow::Cow::Owned(vec![MIME.to_string()])
    }

    fn as_bytes(&self, _: &str) -> Option<std::borrow::Cow<'static, [u8]>> {
        Some(std::borrow::Cow::Borrowed("rectangle".as_bytes()))
    }
}

#[repr(u8)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragState {
    #[default]
    None,
    NW,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
}

impl From<u8> for DragState {
    fn from(state: u8) -> Self {
        match state {
            0 => DragState::None,
            1 => DragState::NW,
            2 => DragState::N,
            3 => DragState::NE,
            4 => DragState::E,
            5 => DragState::SE,
            6 => DragState::S,
            7 => DragState::SW,
            8 => DragState::W,
            _ => unreachable!(),
        }
    }
}

const EDGE_GRAB_THICKNESS: f32 = 8.0;
const CORNER_DIAMETER: f32 = 16.0;

pub struct RectangleSelection<Msg> {
    output_rect: Rect,
    rectangle_selection: Rect,
    window_id: iced_core::window::Id,
    on_rectangle: Box<dyn Fn(DragState, Rect) -> Msg>,
    on_ocr: Msg,
    on_ocr_copy: Msg,
    on_qr: Msg,
    on_qr_copy: Msg,
    has_ocr_text: bool,
    has_qr_codes: bool,
    drag_state: DragState,
    widget_id: widget::Id,
    drag_id: u128,
}

impl<Msg: Clone> RectangleSelection<Msg> {
    pub fn new(
        output_rect: Rect,
        rectangle_selection: Rect,
        drag_direction: DragState,
        window_id: iced_core::window::Id,
        drag_id: u128,
        on_rectangle: impl Fn(DragState, Rect) -> Msg + 'static,
        on_ocr: Msg,
        on_ocr_copy: Msg,
        on_qr: Msg,
        on_qr_copy: Msg,
        has_ocr_text: bool,
        has_qr_codes: bool,
    ) -> Self {
        Self {
            on_rectangle: Box::new(on_rectangle),
            on_ocr,
            on_ocr_copy,
            on_qr,
            on_qr_copy,
            has_ocr_text,
            has_qr_codes,
            drag_state: drag_direction,
            rectangle_selection,
            output_rect,
            window_id,
            drag_id,
            widget_id: widget::Id::new(format!("rectangle-selection-{window_id:?}")),
        }
    }

    pub fn translated_inner_rect(&self) -> Rectangle {
        let inner_rect = self.rectangle_selection;
        let inner_rect = Rectangle::new(
            Point::new(inner_rect.left as f32, inner_rect.top as f32),
            Size::new(
                (inner_rect.right - inner_rect.left).abs() as f32,
                (inner_rect.bottom - inner_rect.top).abs() as f32,
            ),
        );
        Rectangle::new(
            Point::new(
                inner_rect.x - self.output_rect.left as f32,
                inner_rect.y - self.output_rect.top as f32,
            ),
            inner_rect.size(),
        )
    }

    fn drag_state(&self, cursor: mouse::Cursor) -> DragState {
        let inner_rect = self.translated_inner_rect();

        let nw_corner_rect = Rectangle::new(
            Point::new(
                inner_rect.x - CORNER_DIAMETER / 2.0,
                inner_rect.y - CORNER_DIAMETER / 2.0,
            ),
            Size::new(CORNER_DIAMETER, CORNER_DIAMETER),
        );
        if cursor.is_over(nw_corner_rect) {
            return DragState::NW;
        };

        let ne_corner_rect = Rectangle::new(
            Point::new(
                inner_rect.x + inner_rect.width - CORNER_DIAMETER / 2.0,
                inner_rect.y - CORNER_DIAMETER / 2.0,
            ),
            Size::new(CORNER_DIAMETER, CORNER_DIAMETER),
        );
        if cursor.is_over(ne_corner_rect) {
            return DragState::NE;
        };

        let sw_corner_rect = Rectangle::new(
            Point::new(
                inner_rect.x - CORNER_DIAMETER / 2.0,
                inner_rect.y + inner_rect.height - CORNER_DIAMETER / 2.0,
            ),
            Size::new(CORNER_DIAMETER, CORNER_DIAMETER),
        );
        if cursor.is_over(sw_corner_rect) {
            return DragState::SW;
        };

        let se_corner_rect = Rectangle::new(
            Point::new(
                inner_rect.x + inner_rect.width - CORNER_DIAMETER / 2.,
                inner_rect.y + inner_rect.height - CORNER_DIAMETER / 2.,
            ),
            Size::new(CORNER_DIAMETER, CORNER_DIAMETER),
        );
        if cursor.is_over(se_corner_rect) {
            return DragState::SE;
        };

        let n_edge_rect = Rectangle::new(
            Point::new(inner_rect.x, inner_rect.y - EDGE_GRAB_THICKNESS / 2.0),
            Size::new(inner_rect.width, EDGE_GRAB_THICKNESS),
        );
        if cursor.is_over(n_edge_rect) {
            return DragState::N;
        };

        let s_edge_rect = Rectangle::new(
            Point::new(
                inner_rect.x,
                inner_rect.y + inner_rect.height - EDGE_GRAB_THICKNESS / 2.0,
            ),
            Size::new(inner_rect.width, EDGE_GRAB_THICKNESS),
        );
        if cursor.is_over(s_edge_rect) {
            return DragState::S;
        };

        let w_edge_rect = Rectangle::new(
            Point::new(inner_rect.x - EDGE_GRAB_THICKNESS / 2.0, inner_rect.y),
            Size::new(EDGE_GRAB_THICKNESS, inner_rect.height),
        );
        if cursor.is_over(w_edge_rect) {
            return DragState::W;
        };

        let e_edge_rect = Rectangle::new(
            Point::new(
                inner_rect.x + inner_rect.width - EDGE_GRAB_THICKNESS / 2.0,
                inner_rect.y,
            ),
            Size::new(EDGE_GRAB_THICKNESS, inner_rect.height),
        );
        if cursor.is_over(e_edge_rect) {
            return DragState::E;
        };
        DragState::None
    }

    /// Calculate OCR button bounds in widget-local coordinates
    /// Returns None if button shouldn't be shown
    fn ocr_button_bounds(&self) -> Option<Rectangle> {
        let sel = self.rectangle_selection;
        let width = (sel.right - sel.left).abs() as f32;
        let height = (sel.bottom - sel.top).abs() as f32;
        
        if width <= 10.0 || height <= 10.0 {
            return None;
        }
        
        // Convert to widget-local coordinates (subtract output offset)
        let outer_x = self.output_rect.left as f32;
        let outer_y = self.output_rect.top as f32;
        let outer_width = (self.output_rect.right - self.output_rect.left).abs() as f32;
        
        // Rectangle in widget-local coordinates
        let rect_left = sel.left as f32 - outer_x;
        let rect_right = sel.right as f32 - outer_x;
        let rect_top = sel.top as f32 - outer_y;
        
        let button_width = 50.0_f32;
        let button_height = 28.0_f32;
        let margin = 8.0_f32;
        
        let min_rect_width_for_inside = button_width + margin * 2.0;
        let min_rect_height_for_inside = button_height + margin * 2.0;
        
        let (button_x, button_y) = if rect_right + margin + button_width <= outer_width {
            // Right side outside
            (rect_right + margin, rect_top)
        } else if width >= min_rect_width_for_inside 
            && height >= min_rect_height_for_inside {
            // Inside at top-right
            (rect_right - button_width - margin, rect_top + margin)
        } else if rect_left - margin - button_width >= 0.0 {
            // Left side outside
            (rect_left - button_width - margin, rect_top)
        } else {
            // Fallback: inside at top-left
            (rect_left + margin, rect_top + margin)
        };
        
        // Check bounds are valid
        if button_x < 0.0 || button_y < 0.0 {
            return None;
        }
        
        Some(Rectangle::new(
            Point::new(button_x, button_y),
            Size::new(button_width, button_height),
        ))
    }
    
    /// Check if cursor is over the OCR button
    fn is_over_ocr_button(&self, cursor: mouse::Cursor) -> bool {
        if let Some(bounds) = self.ocr_button_bounds() {
            cursor.is_over(bounds)
        } else {
            false
        }
    }
    
    /// Calculate QR button bounds (positioned below OCR button)
    fn qr_button_bounds(&self) -> Option<Rectangle> {
        let ocr_bounds = self.ocr_button_bounds()?;
        let button_spacing = 4.0_f32;
        
        Some(Rectangle::new(
            Point::new(ocr_bounds.x, ocr_bounds.y + ocr_bounds.height + button_spacing),
            Size::new(ocr_bounds.width, ocr_bounds.height),
        ))
    }
    
    /// Check if cursor is over the QR button
    fn is_over_qr_button(&self, cursor: mouse::Cursor) -> bool {
        if let Some(bounds) = self.qr_button_bounds() {
            cursor.is_over(bounds)
        } else {
            false
        }
    }

    fn handle_drag_pos(&mut self, x: i32, y: i32, shell: &mut iced_core::Shell<'_, Msg>) {
        let prev = self.rectangle_selection;

        let d_x = self.output_rect.left + x;
        let d_y = self.output_rect.top + y;

        let prev_state = self.drag_state;
        let reflection_point = match prev_state {
            DragState::None => return,
            DragState::NW => (prev.right, prev.bottom),
            DragState::N => (0, prev.bottom),
            DragState::NE => (prev.left, prev.bottom),
            DragState::E => (prev.left, 0),
            DragState::SE => (prev.left, prev.top),
            DragState::S => (0, prev.top),
            DragState::SW => (prev.right, prev.top),
            DragState::W => (prev.right, 0),
        };

        let new_drag_state = match prev_state {
            DragState::SE | DragState::NW | DragState::NE | DragState::SW => {
                if d_x < reflection_point.0 && d_y < reflection_point.1 {
                    DragState::NW
                } else if d_x > reflection_point.0 && d_y > reflection_point.1 {
                    DragState::SE
                } else if d_x > reflection_point.0 && d_y < reflection_point.1 {
                    DragState::NE
                } else if d_x < reflection_point.0 && d_y > reflection_point.1 {
                    DragState::SW
                } else {
                    prev_state
                }
            }
            DragState::N | DragState::S => {
                if d_y < reflection_point.1 {
                    DragState::N
                } else {
                    DragState::S
                }
            }
            DragState::E | DragState::W => {
                if d_x > reflection_point.0 {
                    DragState::E
                } else {
                    DragState::W
                }
            }

            DragState::None => DragState::None,
        };
        let top_left = match new_drag_state {
            DragState::NW => (d_x, d_y),
            DragState::NE => (reflection_point.0, d_y),
            DragState::SE => (reflection_point.0, reflection_point.1),
            DragState::SW => (d_x, reflection_point.1),
            DragState::N => (prev.left, d_y),
            DragState::E => (reflection_point.0, prev.top),
            DragState::S => (prev.left, reflection_point.1),
            DragState::W => (d_x, prev.top),
            DragState::None => (prev.left, prev.top),
        };

        let bottom_right = match new_drag_state {
            DragState::NW => (reflection_point.0, reflection_point.1),
            DragState::NE => (d_x, reflection_point.1),
            DragState::SE => (d_x, d_y),
            DragState::SW => (reflection_point.0, d_y),
            DragState::N => (prev.right, reflection_point.1),
            DragState::E => (d_x, prev.bottom),
            DragState::S => (prev.right, d_y),
            DragState::W => (reflection_point.0, prev.bottom),
            DragState::None => (prev.right, prev.bottom),
        };
        let new_rect = Rect {
            left: top_left.0,
            top: top_left.1,
            right: bottom_right.0,
            bottom: bottom_right.1,
        };
        self.rectangle_selection = new_rect;
        self.drag_state = new_drag_state;

        shell.publish((self.on_rectangle)(new_drag_state, new_rect));
    }
}

impl<Msg: 'static + Clone> Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for RectangleSelection<Msg>
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &self,
        _tree: &mut cosmic::iced_core::widget::Tree,
        _renderer: &cosmic::Renderer,
        limits: &cosmic::iced_core::layout::Limits,
    ) -> cosmic::iced_core::layout::Node {
        Node::new(limits.width(Length::Fill).height(Length::Fill).resolve(
            Length::Fill,
            Length::Fill,
            cosmic::iced_core::Size::ZERO,
        ))
    }

    fn tag(&self) -> iced_core::widget::tree::Tag {
        struct MyState;
        iced_core::widget::tree::Tag::of::<MyState>()
    }

    fn mouse_interaction(
        &self,
        _state: &iced_core::widget::Tree,
        _layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> iced_core::mouse::Interaction {
        // Check OCR/QR buttons first
        if self.is_over_ocr_button(cursor) || self.is_over_qr_button(cursor) {
            return iced_core::mouse::Interaction::Pointer;
        }
        
        match self.drag_state(cursor) {
            DragState::None => {
                if self.drag_state == DragState::None {
                    iced_core::mouse::Interaction::Crosshair
                } else {
                    iced_core::mouse::Interaction::Grabbing
                }
            }
            DragState::NW | DragState::NE | DragState::SE | DragState::SW => {
                if self.drag_state == DragState::None {
                    iced_core::mouse::Interaction::Grab
                } else {
                    iced_core::mouse::Interaction::Grabbing
                }
            }
            DragState::N | DragState::S => iced_core::mouse::Interaction::ResizingVertically,
            DragState::E | DragState::W => iced_core::mouse::Interaction::ResizingHorizontally,
        }
    }

    fn on_event(
        &mut self,
        _state: &mut iced_core::widget::Tree,
        event: iced_core::Event,
        layout: iced_core::Layout<'_>,
        cursor: iced_core::mouse::Cursor,
        _renderer: &cosmic::Renderer,
        clipboard: &mut dyn iced_core::Clipboard,
        shell: &mut iced_core::Shell<'_, Msg>,
        _viewport: &Rectangle,
    ) -> iced_core::event::Status {
        match event {
            cosmic::iced_core::Event::Dnd(DndEvent::Offer(id, e)) if id == Some(self.drag_id) => {
                if self.drag_state == DragState::None {
                    return cosmic::iced_core::event::Status::Ignored;
                }
                match e {
                    OfferEvent::Enter { x, y, .. } => {
                        let p = Point::new(x as f32, y as f32);
                        let cursor = mouse::Cursor::Available(p);
                        if !cursor.is_over(layout.bounds()) {
                            return cosmic::iced_core::event::Status::Ignored;
                        }

                        self.handle_drag_pos(x.round() as i32, y.round() as i32, shell);
                        cosmic::iced_core::event::Status::Captured
                    }
                    OfferEvent::Motion { x, y } => {
                        let p = Point::new(x as f32, y as f32);
                        let cursor = mouse::Cursor::Available(p);
                        if !cursor.is_over(layout.bounds()) {
                            return cosmic::iced_core::event::Status::Ignored;
                        }
                        self.handle_drag_pos(x.round() as i32, y.round() as i32, shell);
                        cosmic::iced_core::event::Status::Captured
                    }
                    OfferEvent::Drop => {
                        self.drag_state = DragState::None;
                        shell.publish((self.on_rectangle)(
                            DragState::None,
                            self.rectangle_selection,
                        ));
                        cosmic::iced_core::event::Status::Captured
                    }
                    _ => cosmic::iced_core::event::Status::Ignored,
                }
            }
            cosmic::iced_core::Event::Dnd(DndEvent::Source(e)) => {
                if matches!(
                    e,
                    SourceEvent::Finished | SourceEvent::Cancelled | SourceEvent::Dropped
                ) {
                    self.drag_state = DragState::None;
                    shell.publish((self.on_rectangle)(
                        DragState::None,
                        self.rectangle_selection,
                    ));
                }

                cosmic::iced_core::event::Status::Ignored
            }
            cosmic::iced_core::Event::Mouse(e) => {
                if !cursor.is_over(layout.bounds()) {
                    return cosmic::iced_core::event::Status::Ignored;
                }

                if let iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left) = e {
                    // Check if clicking on OCR button
                    if self.is_over_ocr_button(cursor) {
                        // If we have OCR results, copy and close; otherwise run OCR
                        if self.has_ocr_text {
                            shell.publish(self.on_ocr_copy.clone());
                        } else {
                            shell.publish(self.on_ocr.clone());
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    
                    // Check if clicking on QR button
                    if self.is_over_qr_button(cursor) {
                        // If we have QR codes, copy and close; otherwise run QR detection
                        if self.has_qr_codes {
                            shell.publish(self.on_qr_copy.clone());
                        } else {
                            shell.publish(self.on_qr.clone());
                        }
                        return cosmic::iced_core::event::Status::Captured;
                    }
                    
                    let window_id = self.window_id;

                    clipboard.start_dnd(
                        false,
                        Some(DndSource::Surface(window_id)),
                        None,
                        Box::new(MyData),
                        DndAction::Copy,
                    );

                    let s = self.drag_state(cursor);
                    if let DragState::None = s {
                        let mut pos = cursor.position().unwrap_or_default();
                        pos.x += self.output_rect.left as f32;
                        pos.y += self.output_rect.top as f32;
                        self.drag_state = DragState::SE;
                        shell.publish((self.on_rectangle)(
                            DragState::SE,
                            Rect {
                                left: pos.x as i32,
                                top: pos.y as i32,
                                right: pos.x as i32 + 1,
                                bottom: pos.y as i32 + 1,
                            },
                        ));
                    } else {
                        self.drag_state = s;
                        shell.publish((self.on_rectangle)(s, self.rectangle_selection));
                    }
                    return cosmic::iced_core::event::Status::Captured;
                }
                cosmic::iced_core::event::Status::Captured
            }
            _ => cosmic::iced_core::event::Status::Ignored,
        }
    }

    fn draw(
        &self,
        _tree: &cosmic::iced_core::widget::Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        _style: &cosmic::iced_core::renderer::Style,
        _layout: cosmic::iced_core::Layout<'_>,
        _cursor: cosmic::iced_core::mouse::Cursor,
        _viewport: &cosmic::iced_core::Rectangle,
    ) {
        let cosmic = theme.cosmic();
        let accent = Color::from(cosmic.accent_color());
        let inner_rect = self.rectangle_selection;
        let inner_rect = Rectangle::new(
            Point::new(inner_rect.left as f32, inner_rect.top as f32),
            Size::new(
                (inner_rect.right - inner_rect.left).abs() as f32,
                (inner_rect.bottom - inner_rect.top).abs() as f32,
            ),
        );
        let outer_size = Size::new(
            (self.output_rect.right - self.output_rect.left).abs() as f32,
            (self.output_rect.bottom - self.output_rect.top).abs() as f32,
        );
        let outer_top_left = Point::new(self.output_rect.left as f32, self.output_rect.top as f32);
        let outer_rect = Rectangle::new(outer_top_left, outer_size);
        let Some(clipped_inner_rect) = inner_rect.intersection(&outer_rect) else {
            return;
        };
        #[cfg(feature = "wgpu")]
        {
            use cosmic::iced_core::Transformation;
            use cosmic::iced_widget::graphics::{
                Mesh,
                color::pack,
                mesh::{Indexed, SolidVertex2D},
            };
            let mut overlay = Color::BLACK;
            overlay.a = 0.3;

            let outer_bottom_right = (outer_size.width, outer_size.height);
            let inner_top_left = (inner_rect.x, inner_rect.y);
            let outer_top_left = (outer_rect.x, outer_rect.y);
            let inner_bottom_right = (
                inner_rect.x + inner_rect.width,
                inner_rect.y + inner_rect.height,
            );
            let vertices = vec![
                outer_top_left,
                (outer_bottom_right.0, outer_top_left.1),
                outer_bottom_right,
                (outer_top_left.0, outer_bottom_right.1),
                inner_top_left,
                (inner_bottom_right.0, inner_top_left.1),
                inner_bottom_right,
                (inner_top_left.0, inner_bottom_right.1),
            ];
            #[rustfmt::skip]
            let indices = vec![
                5, 2, 1,
                5, 6, 2,
                6, 4, 2,
                6, 8, 4,
                8, 3, 4,
                8, 7, 3,
                7, 1, 3,
                7, 5, 1,
            ];

            renderer.draw_mesh(Mesh::Solid {
                buffers: Indexed {
                    vertices: vertices
                        .into_iter()
                        .map(|v| SolidVertex2D {
                            position: [v.0, v.1],
                            color: pack(overlay),
                        })
                        .collect(),
                    indices,
                },
                transformation: Transformation::IDENTITY,
                clip_bounds: Rectangle::new(Point::ORIGIN, outer_size),
            })
        }

        let translated_clipped_inner_rect = Rectangle::new(
            Point::new(
                clipped_inner_rect.x - outer_rect.x,
                clipped_inner_rect.y - outer_rect.y,
            ),
            clipped_inner_rect.size(),
        );
        let quad = Quad {
            bounds: translated_clipped_inner_rect,
            border: Border {
                radius: 0.0.into(),
                width: 4.0,
                color: accent,
            },
            shadow: Shadow::default(),
        };
        renderer.fill_quad(quad, Color::TRANSPARENT);

        let radius_s = cosmic.radius_s();
        for (x, y) in &[
            (inner_rect.x, inner_rect.y),
            (inner_rect.x + inner_rect.width, inner_rect.y),
            (inner_rect.x, inner_rect.y + inner_rect.height),
            (
                inner_rect.x + inner_rect.width,
                inner_rect.y + inner_rect.height,
            ),
        ] {
            if !outer_rect.contains(Point::new(*x, *y)) {
                continue;
            }
            let translated_x = x - outer_rect.x;
            let translated_y = y - outer_rect.y;
            let bounds = Rectangle::new(
                Point::new(
                    translated_x - CORNER_DIAMETER / 2.0,
                    translated_y - CORNER_DIAMETER / 2.0,
                ),
                Size::new(CORNER_DIAMETER, CORNER_DIAMETER),
            );
            let quad = Quad {
                bounds,
                border: Border {
                    radius: radius_s.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
            };
            renderer.fill_quad(quad, accent);
        }

        // Draw OCR button using the same bounds calculation as hit testing
        if let Some(button_rect) = self.ocr_button_bounds() {
            use cosmic::iced_core::alignment;
            use cosmic::iced_core::text::{Renderer as TextRenderer, Text};
            
            // Show copy icon if we have OCR results, otherwise "OCR"
            let (button_text, font_size) = if self.has_ocr_text { 
                ("📋", 16.0_f32)  // Clipboard emoji as copy icon
            } else { 
                ("OCR", 14.0_f32)
            };
            let border_color = if self.has_ocr_text { 
                Color::from_rgb(0.2, 0.8, 0.2) // Green when results available
            } else { 
                accent 
            };
            
            // Check if button is within screen bounds
            if button_rect.x >= 0.0 && button_rect.y >= 0.0 
                && button_rect.x + button_rect.width <= outer_size.width
                && button_rect.y + button_rect.height <= outer_size.height {
                
                // Draw button background
                let button_quad = Quad {
                    bounds: button_rect,
                    border: Border {
                        radius: radius_s.into(),
                        width: 2.0,
                        color: border_color,
                    },
                    shadow: Shadow::default(),
                };
                renderer.fill_quad(button_quad, Color::from_rgba(0.0, 0.0, 0.0, 0.85));
                
                // Draw button text/icon
                let text = Text {
                    content: button_text.to_string(),
                    bounds: Size::new(button_rect.width, button_rect.height),
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
                    Point::new(
                        button_rect.x + button_rect.width / 2.0,
                        button_rect.y + button_rect.height / 2.0,
                    ),
                    Color::WHITE,
                    Rectangle::new(Point::ORIGIN, outer_size),
                );
            }
        }
        
        // Draw QR button below OCR button
        if let Some(button_rect) = self.qr_button_bounds() {
            use cosmic::iced_core::alignment;
            use cosmic::iced_core::text::{Renderer as TextRenderer, Text};
            
            // Show copy icon if we have QR results, otherwise "QR"
            let (button_text, font_size) = if self.has_qr_codes { 
                ("📋", 16.0_f32)  // Clipboard emoji as copy icon
            } else { 
                ("QR", 14.0_f32)
            };
            let border_color = if self.has_qr_codes { 
                Color::from_rgb(0.2, 0.8, 0.2) // Green when results available
            } else { 
                accent 
            };
            
            // Check if button is within screen bounds
            if button_rect.x >= 0.0 && button_rect.y >= 0.0 
                && button_rect.x + button_rect.width <= outer_size.width
                && button_rect.y + button_rect.height <= outer_size.height {
                
                // Draw button background
                let button_quad = Quad {
                    bounds: button_rect,
                    border: Border {
                        radius: radius_s.into(),
                        width: 2.0,
                        color: border_color,
                    },
                    shadow: Shadow::default(),
                };
                renderer.fill_quad(button_quad, Color::from_rgba(0.0, 0.0, 0.0, 0.85));
                
                // Draw button text/icon
                let text = Text {
                    content: button_text.to_string(),
                    bounds: Size::new(button_rect.width, button_rect.height),
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
                    Point::new(
                        button_rect.x + button_rect.width / 2.0,
                        button_rect.y + button_rect.height / 2.0,
                    ),
                    Color::WHITE,
                    Rectangle::new(Point::ORIGIN, outer_size),
                );
            }
        }
    }

    fn drag_destinations(
        &self,
        _state: &iced_core::widget::Tree,
        layout: iced_core::Layout<'_>,
        _renderer: &cosmic::Renderer,
        dnd_rectangles: &mut iced_core::clipboard::DndDestinationRectangles,
    ) {
        let bounds = layout.bounds();
        dnd_rectangles.push(DndDestinationRectangle {
            id: self.drag_id,
            rectangle: dnd::Rectangle {
                x: bounds.x as f64,
                y: bounds.y as f64,
                width: bounds.width as f64,
                height: bounds.height as f64,
            },
            mime_types: vec![Cow::Borrowed(MIME)],
            actions: DndAction::Copy,
            preferred: DndAction::Copy,
        });
    }

    fn set_id(&mut self, id: widget::Id) {
        self.widget_id = id;
    }
}

impl<'a, Message> From<RectangleSelection<Message>> for cosmic::Element<'a, Message>
where
    Message: 'static + Clone,
{
    fn from(w: RectangleSelection<Message>) -> cosmic::Element<'a, Message> {
        cosmic::Element::new(w)
    }
}

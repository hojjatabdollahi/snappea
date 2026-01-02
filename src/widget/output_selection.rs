use cosmic::{
    iced::Limits,
    iced_core::{
        Background, Border, Color, Length, Renderer, Shadow, Size, Point,
        layout::Node,
        mouse,
        renderer::Quad,
        widget::{
            Tree,
            tree::{self, State},
        },
        text::Renderer as TextRenderer,
    },
    widget::Widget,
};

pub struct OutputSelection<Msg> {
    on_enter: Msg,
    on_ocr: Msg,
    on_ocr_copy: Msg,
    on_qr: Msg,
    on_qr_copy: Msg,
    on_arrow_toggle: Msg,
    has_ocr_text: bool,
    has_qr_codes: bool,
    arrow_mode: bool,
    annotate_mode: bool,
}

impl<Msg> OutputSelection<Msg> {
    pub fn new(
        on_enter: Msg,
        on_ocr: Msg,
        on_ocr_copy: Msg,
        on_qr: Msg,
        on_qr_copy: Msg,
        on_arrow_toggle: Msg,
        has_ocr_text: bool,
        has_qr_codes: bool,
        arrow_mode: bool,
        annotate_mode: bool,
    ) -> Self {
        Self {
            on_enter,
            on_ocr,
            on_ocr_copy,
            on_qr,
            on_qr_copy,
            on_arrow_toggle,
            has_ocr_text,
            has_qr_codes,
            arrow_mode,
            annotate_mode,
        }
    }

    /// Calculate button bounds for OCR button (bottom center, left side)
    fn ocr_button_bounds(&self, bounds: cosmic::iced_core::Rectangle) -> cosmic::iced_core::Rectangle {
        let button_width = 64.0;
        let button_height = 32.0;
        cosmic::iced_core::Rectangle {
            x: bounds.x + (bounds.width / 2.0) - button_width - 8.0,
            y: bounds.y + bounds.height - button_height - 24.0,
            width: button_width,
            height: button_height,
        }
    }

    /// Calculate button bounds for QR button (bottom center, right side)
    fn qr_button_bounds(&self, bounds: cosmic::iced_core::Rectangle) -> cosmic::iced_core::Rectangle {
        let button_width = 64.0;
        let button_height = 32.0;
        cosmic::iced_core::Rectangle {
            x: bounds.x + (bounds.width / 2.0) + 8.0,
            y: bounds.y + bounds.height - button_height - 24.0,
            width: button_width,
            height: button_height,
        }
    }

    /// Calculate button bounds for arrow button (left of OCR button)
    fn arrow_button_bounds(&self, bounds: cosmic::iced_core::Rectangle) -> Option<cosmic::iced_core::Rectangle> {
        if !self.annotate_mode {
            return None;
        }
        let button_width = 32.0;
        let button_height = 32.0;
        Some(cosmic::iced_core::Rectangle {
            x: bounds.x + (bounds.width / 2.0) - 64.0 - button_width - 16.0,
            y: bounds.y + bounds.height - button_height - 24.0,
            width: button_width,
            height: button_height,
        })
    }
}

impl<Msg: Clone + 'static> Widget<Msg, cosmic::Theme, cosmic::Renderer> for OutputSelection<Msg> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn state(&self) -> cosmic::iced_core::widget::tree::State {
        State::new(MyState::default())
    }

    fn tag(&self) -> cosmic::iced_core::widget::tree::Tag {
        tree::Tag::of::<MyState>()
    }

    fn layout(&self, _tree: &mut Tree, _renderer: &cosmic::Renderer, limits: &Limits) -> Node {
        let limits = limits.width(Length::Fill).height(Length::Fill);
        Node::new(limits.resolve(Length::Fill, Length::Fill, Size::ZERO))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        _style: &cosmic::iced_core::renderer::Style,
        layout: cosmic::iced_core::Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
    ) {
        let cosmic_theme = theme.cosmic();
        let radius_s = cosmic_theme.radius_s();
        let mut accent = Color::from(cosmic_theme.accent_color());
        let should_draw = {
            let my_state = tree.state.downcast_ref::<MyState>();
            my_state.hovered || my_state.focused
        };

        let bounds = layout.bounds();

        if should_draw {
            accent.a = 0.7;
            renderer.fill_quad(
                Quad {
                    bounds,
                    border: Border {
                        radius: radius_s.into(),
                        width: 12.0,
                        color: accent,
                    },
                    shadow: Shadow::default(),
                },
                Background::Color(Color::TRANSPARENT),
            );

            accent.a = 1.0;

            renderer.fill_quad(
                Quad {
                    bounds,
                    border: Border {
                        radius: radius_s.into(),
                        width: 4.0,
                        color: accent,
                    },
                    ..Default::default()
                },
                Background::Color(Color::TRANSPARENT),
            );
        }

        // Always draw buttons at the bottom
        accent.a = 1.0;
        
        // Draw OCR button
        let ocr_bounds = self.ocr_button_bounds(bounds);
        let ocr_hovered = cursor.position().map(|p| ocr_bounds.contains(p)).unwrap_or(false);
        let ocr_text = if self.has_ocr_text { "📋" } else { "OCR" };
        let ocr_bg = if ocr_hovered {
            cosmic_theme.accent_color()
        } else {
            cosmic_theme.background.component.base
        };
        
        renderer.fill_quad(
            Quad {
                bounds: ocr_bounds,
                border: Border {
                    radius: radius_s.into(),
                    width: 2.0,
                    color: accent,
                },
                shadow: Shadow::default(),
            },
            Background::Color(ocr_bg.into()),
        );
        
        renderer.fill_text(
            cosmic::iced_core::text::Text {
                content: ocr_text.into(),
                bounds: Size::new(ocr_bounds.width, ocr_bounds.height),
                size: cosmic::iced::Pixels(14.0),
                line_height: cosmic::iced_core::text::LineHeight::Relative(1.2),
                font: cosmic::iced::Font::default(),
                horizontal_alignment: cosmic::iced_core::alignment::Horizontal::Center,
                vertical_alignment: cosmic::iced_core::alignment::Vertical::Center,
                shaping: cosmic::iced_core::text::Shaping::Advanced,
                wrapping: cosmic::iced_core::text::Wrapping::None,
            },
            Point::new(ocr_bounds.x, ocr_bounds.y),
            cosmic_theme.on_accent_color().into(),
            *viewport,
        );
        
        // Draw QR button
        let qr_bounds = self.qr_button_bounds(bounds);
        let qr_hovered = cursor.position().map(|p| qr_bounds.contains(p)).unwrap_or(false);
        let qr_text = if self.has_qr_codes { "📋" } else { "QR" };
        let qr_bg = if qr_hovered {
            cosmic_theme.accent_color()
        } else {
            cosmic_theme.background.component.base
        };
        
        renderer.fill_quad(
            Quad {
                bounds: qr_bounds,
                border: Border {
                    radius: radius_s.into(),
                    width: 2.0,
                    color: accent,
                },
                shadow: Shadow::default(),
            },
            Background::Color(qr_bg.into()),
        );
        
        renderer.fill_text(
            cosmic::iced_core::text::Text {
                content: qr_text.into(),
                bounds: Size::new(qr_bounds.width, qr_bounds.height),
                size: cosmic::iced::Pixels(14.0),
                line_height: cosmic::iced_core::text::LineHeight::Relative(1.2),
                font: cosmic::iced::Font::default(),
                horizontal_alignment: cosmic::iced_core::alignment::Horizontal::Center,
                vertical_alignment: cosmic::iced_core::alignment::Vertical::Center,
                shaping: cosmic::iced_core::text::Shaping::Advanced,
                wrapping: cosmic::iced_core::text::Wrapping::None,
            },
            Point::new(qr_bounds.x, qr_bounds.y),
            cosmic_theme.on_accent_color().into(),
            *viewport,
        );
        
        // Draw arrow button if annotate mode is enabled
        if let Some(arrow_bounds) = self.arrow_button_bounds(bounds) {
            let arrow_hovered = cursor.position().map(|p| arrow_bounds.contains(p)).unwrap_or(false);
            let arrow_bg = if self.arrow_mode {
                cosmic_theme.accent_color()
            } else if arrow_hovered {
                cosmic_theme.background.component.hover
            } else {
                cosmic_theme.background.component.base
            };
            
            renderer.fill_quad(
                Quad {
                    bounds: arrow_bounds,
                    border: Border {
                        radius: radius_s.into(),
                        width: 2.0,
                        color: accent,
                    },
                    shadow: Shadow::default(),
                },
                Background::Color(arrow_bg.into()),
            );
            
            renderer.fill_text(
                cosmic::iced_core::text::Text {
                    content: "→".into(),
                    bounds: Size::new(arrow_bounds.width, arrow_bounds.height),
                    size: cosmic::iced::Pixels(18.0),
                    line_height: cosmic::iced_core::text::LineHeight::Relative(1.2),
                    font: cosmic::iced::Font::default(),
                    horizontal_alignment: cosmic::iced_core::alignment::Horizontal::Center,
                    vertical_alignment: cosmic::iced_core::alignment::Vertical::Center,
                    shaping: cosmic::iced_core::text::Shaping::Advanced,
                    wrapping: cosmic::iced_core::text::Wrapping::None,
                },
                Point::new(arrow_bounds.x, arrow_bounds.y),
                cosmic_theme.on_accent_color().into(),
                *viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Tree,
        layout: cosmic::iced_core::Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        _viewport: &cosmic::iced_core::Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> cosmic::iced_core::mouse::Interaction {
        let bounds = layout.bounds();
        
        if let Some(pos) = cursor.position() {
            let ocr_bounds = self.ocr_button_bounds(bounds);
            let qr_bounds = self.qr_button_bounds(bounds);
            
            if ocr_bounds.contains(pos) || qr_bounds.contains(pos) {
                return cosmic::iced_core::mouse::Interaction::Pointer;
            }
            
            if let Some(arrow_bounds) = self.arrow_button_bounds(bounds) {
                if arrow_bounds.contains(pos) {
                    return cosmic::iced_core::mouse::Interaction::Pointer;
                }
            }
        }
        
        cosmic::iced_core::mouse::Interaction::default()
    }

    fn on_event(
        &mut self,
        state: &mut Tree,
        event: cosmic::iced_core::Event,
        layout: cosmic::iced_core::Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        _renderer: &cosmic::Renderer,
        _clipboard: &mut dyn cosmic::iced_core::Clipboard,
        shell: &mut cosmic::iced_core::Shell<'_, Msg>,
        _viewport: &cosmic::iced_core::Rectangle,
    ) -> cosmic::iced_core::event::Status {
        let my_state = state.state.downcast_mut::<MyState>();
        let hovered = cursor.is_over(layout.bounds());
        let changed = my_state.hovered != hovered;
        my_state.hovered = hovered;

        let bounds = layout.bounds();

        // Handle button clicks
        if let cosmic::iced_core::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if let Some(pos) = cursor.position() {
                let ocr_bounds = self.ocr_button_bounds(bounds);
                let qr_bounds = self.qr_button_bounds(bounds);
                
                if ocr_bounds.contains(pos) {
                    if self.has_ocr_text {
                        shell.publish(self.on_ocr_copy.clone());
                    } else {
                        shell.publish(self.on_ocr.clone());
                    }
                    return cosmic::iced_core::event::Status::Captured;
                }
                
                if qr_bounds.contains(pos) {
                    if self.has_qr_codes {
                        shell.publish(self.on_qr_copy.clone());
                    } else {
                        shell.publish(self.on_qr.clone());
                    }
                    return cosmic::iced_core::event::Status::Captured;
                }
                
                if let Some(arrow_bounds) = self.arrow_button_bounds(bounds) {
                    if arrow_bounds.contains(pos) {
                        shell.publish(self.on_arrow_toggle.clone());
                        return cosmic::iced_core::event::Status::Captured;
                    }
                }
            }
        }

        if changed {
            match event {
                cosmic::iced_core::Event::Mouse(mouse::Event::CursorMoved { .. })
                | cosmic::iced_core::Event::Mouse(mouse::Event::CursorEntered) => {
                    shell.publish(self.on_enter.clone());
                    return cosmic::iced_core::event::Status::Captured;
                }
                _ => {}
            };
        };

        cosmic::iced_core::event::Status::Ignored
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MyState {
    pub hovered: bool,
    pub focused: bool,
}

impl<'a, Message> From<OutputSelection<Message>> for cosmic::Element<'a, Message>
where
    Message: 'static + Clone,
{
    fn from(w: OutputSelection<Message>) -> cosmic::Element<'a, Message> {
        cosmic::Element::new(w)
    }
}

use cosmic::{
    iced::Limits,
    iced_core::{
        Background, Border, Color, Length, Renderer, Shadow, Size, alignment,
        layout::Node,
        mouse,
        renderer::Quad,
        text::{self, Renderer as TextRenderer},
        widget::{
            Tree,
            tree::{self, State},
        },
    },
    widget::Widget,
};

pub struct OutputSelection<Msg> {
    on_enter: Msg,
    on_click: Option<Msg>,
    /// Whether this output is currently selected/confirmed (will always draw the selection frame)
    selected: bool,
    /// Whether we're in picker mode (choosing a screen)
    picker_mode: bool,
    /// Whether this output is focused (highlighted) in picker mode
    focused: bool,
}

impl<Msg> OutputSelection<Msg> {
    pub fn new(on_enter: Msg) -> Self {
        Self {
            on_enter,
            on_click: None,
            selected: false,
            picker_mode: false,
            focused: false,
        }
    }

    /// Mark this output as selected/confirmed (will always draw the selection frame)
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Set picker mode (true = choosing, false = confirmed)
    pub fn picker_mode(mut self, picker_mode: bool) -> Self {
        self.picker_mode = picker_mode;
        self
    }

    /// Mark this output as focused (highlighted in picker mode)
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set the message to publish when clicked (for confirming selection)
    pub fn on_click(mut self, msg: Msg) -> Self {
        self.on_click = Some(msg);
        self
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
        _tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        _style: &cosmic::iced_core::renderer::Style,
        layout: cosmic::iced_core::Layout<'_>,
        _cursor: cosmic::iced_core::mouse::Cursor,
        _viewport: &cosmic::iced_core::Rectangle,
    ) {
        let cosmic_theme = theme.cosmic();
        let radius_s = cosmic_theme.radius_s();
        let mut accent = Color::from(cosmic_theme.accent_color());
        let bounds = layout.bounds();

        // Determine if we should draw the selection frame
        let should_draw_frame = if self.picker_mode {
            // In picker mode, draw frame on focused output
            self.focused
        } else {
            // In confirmed mode, draw frame on selected output
            self.selected
        };

        if should_draw_frame {
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

        // In picker mode, draw a hint in the center
        if self.picker_mode {
            // Semi-transparent dark background for the hint
            let hint_width = 400.0_f32;
            let hint_height = 80.0_f32;
            let hint_x = bounds.x + (bounds.width - hint_width) / 2.0;
            let hint_y = bounds.y + (bounds.height - hint_height) / 2.0;

            let hint_bounds = cosmic::iced_core::Rectangle {
                x: hint_x,
                y: hint_y,
                width: hint_width,
                height: hint_height,
            };

            // Draw background
            renderer.fill_quad(
                Quad {
                    bounds: hint_bounds,
                    border: Border {
                        radius: 12.0.into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    shadow: Shadow::default(),
                },
                Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.7)),
            );

            // Draw hint text
            let hint_text = if self.focused {
                "Click or press Enter to select this screen"
            } else {
                "Click to select • Arrow keys to navigate"
            };

            let text_color = Color::WHITE;
            let font = cosmic::iced::Font::default();
            let text_size = cosmic::iced::Pixels(16.0);

            // For centered alignment, position at the center of the hint box
            let text_center_x = hint_x + hint_width / 2.0;
            let text_center_y = hint_y + hint_height / 2.0;

            renderer.fill_text(
                text::Text {
                    content: hint_text.to_string(),
                    bounds: Size::new(hint_width, hint_height),
                    size: text_size,
                    line_height: text::LineHeight::default(),
                    font,
                    horizontal_alignment: alignment::Horizontal::Center,
                    vertical_alignment: alignment::Vertical::Center,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::Word,
                },
                cosmic::iced_core::Point::new(text_center_x, text_center_y),
                text_color,
                hint_bounds,
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
        if self.picker_mode && cursor.is_over(layout.bounds()) {
            cosmic::iced_core::mouse::Interaction::Pointer
        } else {
            cosmic::iced_core::mouse::Interaction::default()
        }
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
        let hover_changed = my_state.hovered != hovered;
        my_state.hovered = hovered;

        // Handle hover for focus tracking (in picker mode)
        // Publish on_enter when:
        // 1. Cursor just entered this output (hover_changed && hovered), OR
        // 2. Cursor is over this output and we haven't published yet (initial detection)
        //    This handles the case where cursor is already over an output when app starts
        let should_publish_enter =
            self.picker_mode && hovered && (hover_changed || !my_state.entered_published);

        if should_publish_enter {
            match event {
                cosmic::iced_core::Event::Mouse(mouse::Event::CursorMoved { .. })
                | cosmic::iced_core::Event::Mouse(mouse::Event::CursorEntered) => {
                    my_state.entered_published = true;
                    shell.publish(self.on_enter.clone());
                    return cosmic::iced_core::event::Status::Captured;
                }
                _ => {}
            };
        }

        // Handle click for confirming selection (in picker mode)
        if self.picker_mode
            && hovered
            && let cosmic::iced_core::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) =
                event
            && let Some(on_click) = &self.on_click
        {
            shell.publish(on_click.clone());
            return cosmic::iced_core::event::Status::Captured;
        }

        cosmic::iced_core::event::Status::Ignored
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MyState {
    pub hovered: bool,
    /// Whether we've published on_enter at least once (to detect initial cursor position)
    pub entered_published: bool,
}

impl<'a, Message> From<OutputSelection<Message>> for cosmic::Element<'a, Message>
where
    Message: 'static + Clone,
{
    fn from(w: OutputSelection<Message>) -> cosmic::Element<'a, Message> {
        cosmic::Element::new(w)
    }
}

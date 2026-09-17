// SPDX-License-Identifier: GPL-3.0-only

use crate::fl;
use cosmic::{
    iced::Limits,
    iced::core::{
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
    pub const fn new(on_enter: Msg) -> Self {
        Self {
            on_enter,
            on_click: None,
            selected: false,
            picker_mode: false,
            focused: false,
        }
    }

    /// Mark this output as selected/confirmed (will always draw the selection frame)
    pub const fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Set picker mode (true = choosing, false = confirmed)
    pub const fn picker_mode(mut self, picker_mode: bool) -> Self {
        self.picker_mode = picker_mode;
        self
    }

    /// Mark this output as focused (highlighted in picker mode)
    pub const fn focused(mut self, focused: bool) -> Self {
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

    fn state(&self) -> cosmic::iced::core::widget::tree::State {
        State::new(MyState::default())
    }

    fn tag(&self) -> cosmic::iced::core::widget::tree::Tag {
        tree::Tag::of::<MyState>()
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &cosmic::Renderer, limits: &Limits) -> Node {
        let limits = limits.width(Length::Fill).height(Length::Fill);
        Node::new(limits.resolve(Length::Fill, Length::Fill, Size::ZERO))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        _style: &cosmic::iced::core::renderer::Style,
        layout: cosmic::iced::core::Layout<'_>,
        _cursor: cosmic::iced::core::mouse::Cursor,
        _viewport: &cosmic::iced::core::Rectangle,
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
                    snap: false,
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

            let hint_bounds = cosmic::iced::core::Rectangle {
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
                    snap: false,
                },
                Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.7)),
            );

            // Draw hint text
            let hint_text = if self.focused {
                fl!("select-screen-hint")
            } else {
                fl!("select-screen-navigate")
            };

            let text_color = Color::WHITE;
            let font = cosmic::iced::Font::default();
            let text_size = cosmic::iced::Pixels(16.0);

            // For centered alignment, position at the center of the hint box
            let text_center_x = hint_x + hint_width / 2.0;
            let text_center_y = hint_y + hint_height / 2.0;

            renderer.fill_text(
                text::Text {
                    content: hint_text,
                    bounds: Size::new(hint_width, hint_height),
                    size: text_size,
                    line_height: text::LineHeight::default(),
                    font,
                    align_x: alignment::Horizontal::Center.into(),
                    align_y: alignment::Vertical::Center,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::Word,
                    ellipsize: text::Ellipsize::default(),
                },
                cosmic::iced::core::Point::new(text_center_x, text_center_y),
                text_color,
                hint_bounds,
            );
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Tree,
        layout: cosmic::iced::core::Layout<'_>,
        cursor: cosmic::iced::core::mouse::Cursor,
        _viewport: &cosmic::iced::core::Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> cosmic::iced::core::mouse::Interaction {
        if self.picker_mode && cursor.is_over(layout.bounds()) {
            cosmic::iced::core::mouse::Interaction::Pointer
        } else {
            cosmic::iced::core::mouse::Interaction::default()
        }
    }

    fn update(
        &mut self,
        state: &mut Tree,
        event: &cosmic::iced::core::Event,
        layout: cosmic::iced::core::Layout<'_>,
        cursor: cosmic::iced::core::mouse::Cursor,
        _renderer: &cosmic::Renderer,
        _clipboard: &mut dyn cosmic::iced::core::Clipboard,
        shell: &mut cosmic::iced::core::Shell<'_, Msg>,
        _viewport: &cosmic::iced::core::Rectangle,
    ) {
        let my_state = state.state.downcast_mut::<MyState>();
        let hovered = cursor.is_over(layout.bounds());
        let hover_changed = my_state.hovered != hovered;
        my_state.hovered = hovered;

        // Publish on_enter when the cursor enters, or on the first frame it is already here.
        let should_publish_enter =
            self.picker_mode && hovered && (hover_changed || !my_state.entered_published);

        if should_publish_enter
            && let cosmic::iced::core::Event::Mouse(
                mouse::Event::CursorMoved { .. } | mouse::Event::CursorEntered,
            ) = event
        {
            my_state.entered_published = true;
            shell.publish(self.on_enter.clone());
            shell.capture_event();
            return;
        }

        // Handle click for confirming selection (in picker mode)
        if self.picker_mode
            && hovered
            && matches!(
                event,
                cosmic::iced::core::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left,))
            )
            && let Some(on_click) = &self.on_click
        {
            shell.publish(on_click.clone());
            shell.capture_event();
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MyState {
    pub hovered: bool,
    /// Whether we've published `on_enter` at least once (to detect initial cursor position)
    pub entered_published: bool,
}

impl<Message> From<OutputSelection<Message>> for cosmic::Element<'_, Message>
where
    Message: 'static + Clone,
{
    fn from(w: OutputSelection<Message>) -> Self {
        cosmic::Element::new(w)
    }
}

//! Widget for selecting toolbar position with triangular hit regions

use cosmic::iced::Size;
use cosmic::iced_core::{
    Background, Border, Element, Length,
    layout::{self, Layout},
    renderer::Quad,
};

use crate::config::ToolbarPosition;

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
    fn get_region(
        &self,
        x: f32,
        y: f32,
        bounds: cosmic::iced_core::Rectangle,
    ) -> Option<ToolbarPosition> {
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
        if local_x < 0.0
            || local_x > extended_bounds.width
            || local_y < 0.0
            || local_y > extended_bounds.height
        {
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
        let diag2 =
            extended_bounds.height - local_x * (extended_bounds.height / extended_bounds.width); // TR to BL

        let above_diag1 = local_y < diag1;
        let above_diag2 = local_y < diag2;

        match (above_diag1, above_diag2) {
            (true, true) => Some(ToolbarPosition::Top), // Above both diagonals
            (false, false) => Some(ToolbarPosition::Bottom), // Below both diagonals
            (true, false) => Some(ToolbarPosition::Right), // Above diag1, below diag2
            (false, true) => Some(ToolbarPosition::Left), // Below diag1, above diag2
        }
    }
}

impl<Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for ToolbarPositionSelector<Msg>
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(
        &self,
        _tree: &mut cosmic::iced_core::widget::Tree,
        _renderer: &cosmic::Renderer,
        _limits: &cosmic::iced::Limits,
    ) -> layout::Node {
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
        let hovered_region = cursor
            .position()
            .and_then(|pos| self.get_region(pos.x, pos.y, bounds));

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
            Quad {
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
            Quad {
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
            Quad {
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
            Quad {
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
        if let Some(pos) = cursor.position()
            && self.get_region(pos.x, pos.y, layout.bounds()).is_some()
        {
            return cosmic::iced_core::mouse::Interaction::Pointer;
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
            && let Some(pos) = cursor.position()
            && let Some(region) = self.get_region(pos.x, pos.y, layout.bounds())
        {
            let msg = match region {
                ToolbarPosition::Top => self.on_top.clone(),
                ToolbarPosition::Bottom => self.on_bottom.clone(),
                ToolbarPosition::Left => self.on_left.clone(),
                ToolbarPosition::Right => self.on_right.clone(),
            };
            shell.publish(msg);
            return cosmic::iced_core::event::Status::Captured;
        }
        cosmic::iced_core::event::Status::Ignored
    }
}

impl<'a, Msg: Clone + 'static> From<ToolbarPositionSelector<Msg>>
    for Element<'a, Msg, cosmic::Theme, cosmic::Renderer>
{
    fn from(widget: ToolbarPositionSelector<Msg>) -> Self {
        Element::new(widget)
    }
}

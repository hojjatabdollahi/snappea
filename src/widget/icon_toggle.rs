//! A toggle widget that displays two icons inside a pill with a selection circle
//!
//! Shows a pill-shaped background with two icons at either end.
//! The selected icon has a circle behind it inside the pill.
//! Supports both horizontal and vertical orientations.
//! Supports smooth animation via cosmic-time.

use cosmic::Element;
use cosmic::iced::Size;
use cosmic::iced_core::{
    Background, Border, Color, Layout, Length, Rectangle, layout,
    mouse::{self, Cursor},
    renderer::Quad,
    widget::Tree,
};
use cosmic::widget::icon;
use cosmic_time::once_cell::sync::Lazy;
use cosmic_time::{Duration, Ease, Exponential, Timeline, chain, lazy, toggler};
use std::rc::Rc;

/// Animation ID for the capture mode toggle
pub static CAPTURE_MODE_TOGGLE_ID: Lazy<cosmic_time::id::Toggler> =
    Lazy::new(cosmic_time::id::Toggler::unique);

/// Animation duration for the toggle in milliseconds
const TOGGLE_ANIM_DURATION_MS: u64 = 180;

/// Get the current animation percent for the capture mode toggle from the timeline
pub fn get_toggle_percent(timeline: &Timeline, is_video_mode: bool) -> f32 {
    timeline
        .get(&CAPTURE_MODE_TOGGLE_ID.clone().into(), 0)
        .map_or(if is_video_mode { 1.0 } else { 0.0 }, |interped| {
            interped.value
        })
}

/// Create an animation chain for toggling to video mode (A -> B)
pub fn toggle_to_video() -> cosmic_time::chain::Toggler {
    chain!(
        CAPTURE_MODE_TOGGLE_ID.clone(),
        lazy::toggler(Duration::ZERO),
        toggler(Duration::from_millis(TOGGLE_ANIM_DURATION_MS))
            .percent(1.0)
            .ease(Ease::Exponential(Exponential::In)),
    )
}

/// Create an animation chain for toggling to screenshot mode (B -> A)
pub fn toggle_to_screenshot() -> cosmic_time::chain::Toggler {
    chain!(
        CAPTURE_MODE_TOGGLE_ID.clone(),
        lazy::toggler(Duration::ZERO),
        toggler(Duration::from_millis(TOGGLE_ANIM_DURATION_MS))
            .percent(0.0)
            .ease(Ease::Exponential(Exponential::In)),
    )
}

// Layout constants
const PILL_THICKNESS: f32 = 38.0; // Slightly larger than toolbar buttons
const CIRCLE_SIZE: f32 = 32.0; // Selection circle inside the pill
const ICON_SIZE: f32 = 24.0; // Larger icons for better visibility
const PILL_LENGTH: f32 = 80.0; // Length of the pill (width if horizontal, height if vertical)

/// A toggle widget that switches between two icons with a pill + circle design
pub struct IconToggle<'a, Msg> {
    /// Icon names for creating sized variants
    icon_a_name: &'a str,
    icon_b_name: &'a str,
    /// Whether the second option (B) is selected
    is_b_selected: bool,
    /// Whether the toggle is vertical (icons stacked) or horizontal (icons side by side)
    is_vertical: bool,
    /// Message to emit when toggled (None = no-op)
    on_toggle: Option<Box<dyn Fn(bool) -> Msg + 'a>>,
    /// Animation percent (0.0 = A selected, 1.0 = B selected)
    /// If None, uses is_b_selected directly (no animation)
    animation_percent: Option<f32>,
}

impl<'a, Msg> IconToggle<'a, Msg> {
    /// Create a new icon toggle widget (horizontal by default, no callback)
    pub fn new(icon_a: &'a str, icon_b: &'a str, is_b_selected: bool) -> Self {
        Self {
            icon_a_name: icon_a,
            icon_b_name: icon_b,
            is_b_selected,
            is_vertical: false,
            on_toggle: None,
            animation_percent: None,
        }
    }

    /// Set the callback for when the toggle is clicked
    pub fn on_toggle(mut self, callback: impl Fn(bool) -> Msg + 'a) -> Self {
        self.on_toggle = Some(Box::new(callback));
        self
    }

    /// Set the toggle to vertical orientation
    pub fn vertical(mut self) -> Self {
        self.is_vertical = true;
        self
    }

    /// Set the animation percent (0.0 = A, 1.0 = B)
    /// This is used for smooth animation transitions
    pub fn percent(mut self, percent: f32) -> Self {
        self.animation_percent = Some(percent.clamp(0.0, 1.0));
        self
    }

    /// Get the effective animation percent for rendering
    fn effective_percent(&self) -> f32 {
        self.animation_percent
            .unwrap_or(if self.is_b_selected { 1.0 } else { 0.0 })
    }

    /// Calculate the total widget width
    fn total_width(&self) -> f32 {
        if self.is_vertical {
            PILL_THICKNESS
        } else {
            PILL_LENGTH
        }
    }

    /// Calculate the total widget height
    fn total_height(&self) -> f32 {
        if self.is_vertical {
            PILL_LENGTH
        } else {
            PILL_THICKNESS
        }
    }

    /// Get bounds for icon A's clickable area
    fn icon_a_bounds(&self, layout_bounds: Rectangle) -> Rectangle {
        if self.is_vertical {
            // Top half
            Rectangle {
                x: layout_bounds.x,
                y: layout_bounds.y,
                width: PILL_THICKNESS,
                height: PILL_LENGTH / 2.0,
            }
        } else {
            // Left half
            Rectangle {
                x: layout_bounds.x,
                y: layout_bounds.y,
                width: PILL_LENGTH / 2.0,
                height: PILL_THICKNESS,
            }
        }
    }

    /// Get bounds for icon B's clickable area
    fn icon_b_bounds(&self, layout_bounds: Rectangle) -> Rectangle {
        if self.is_vertical {
            // Bottom half
            Rectangle {
                x: layout_bounds.x,
                y: layout_bounds.y + PILL_LENGTH / 2.0,
                width: PILL_THICKNESS,
                height: PILL_LENGTH / 2.0,
            }
        } else {
            // Right half
            Rectangle {
                x: layout_bounds.x + PILL_LENGTH / 2.0,
                y: layout_bounds.y,
                width: PILL_LENGTH / 2.0,
                height: PILL_THICKNESS,
            }
        }
    }
}

impl<'a, Msg: Clone + 'a> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for IconToggle<'a, Msg>
{
    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(self.total_width()),
            Length::Fixed(self.total_height()),
        )
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::empty(), Tree::empty()]
    }

    fn diff(&mut self, _tree: &mut Tree) {}

    fn layout(
        &self,
        _tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        limits: &cosmic::iced::Limits,
    ) -> layout::Node {
        let width = self.total_width();
        let height = self.total_height();

        let size = limits
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .resolve(width, height, Size::new(width, height));

        layout::Node::new(size)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        use cosmic::iced_core::Renderer as _;

        let bounds = layout.bounds();
        let cosmic_theme = theme.cosmic();

        // Colors
        let accent_color: Color = cosmic_theme.accent_color().into();
        let pill_color = Color::from_rgba(0.3, 0.3, 0.3, 0.6);
        let hover_color = Color::from_rgba(accent_color.r, accent_color.g, accent_color.b, 0.3);

        // Calculate icon center positions based on orientation
        let (icon_a_center_x, icon_a_center_y, icon_b_center_x, icon_b_center_y) =
            if self.is_vertical {
                let center_x = bounds.x + PILL_THICKNESS / 2.0;
                let icon_a_y = bounds.y + PILL_LENGTH / 4.0;
                let icon_b_y = bounds.y + PILL_LENGTH * 3.0 / 4.0;
                (center_x, icon_a_y, center_x, icon_b_y)
            } else {
                let center_y = bounds.y + PILL_THICKNESS / 2.0;
                let icon_a_x = bounds.x + PILL_LENGTH / 4.0;
                let icon_b_x = bounds.x + PILL_LENGTH * 3.0 / 4.0;
                (icon_a_x, center_y, icon_b_x, center_y)
            };

        // Check hover state
        let icon_a_bounds = self.icon_a_bounds(bounds);
        let icon_b_bounds = self.icon_b_bounds(bounds);
        let hover_a = cursor.position().is_some_and(|p| icon_a_bounds.contains(p));
        let hover_b = cursor.position().is_some_and(|p| icon_b_bounds.contains(p));

        // 1. Draw the pill background (spans entire widget)
        let pill_radius = PILL_THICKNESS / 2.0;
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: bounds.y,
                    width: self.total_width(),
                    height: self.total_height(),
                },
                border: Border {
                    radius: pill_radius.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(pill_color),
        );

        // 2. Draw selection circle - ANIMATED position using percent
        let percent = self.effective_percent();

        // Interpolate circle position based on animation percent
        let (circle_center_x, circle_center_y) = if self.is_vertical {
            // Interpolate Y position
            let y = icon_a_center_y + (icon_b_center_y - icon_a_center_y) * percent;
            (icon_a_center_x, y)
        } else {
            // Interpolate X position
            let x = icon_a_center_x + (icon_b_center_x - icon_a_center_x) * percent;
            (x, icon_a_center_y)
        };

        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: circle_center_x - CIRCLE_SIZE / 2.0,
                    y: circle_center_y - CIRCLE_SIZE / 2.0,
                    width: CIRCLE_SIZE,
                    height: CIRCLE_SIZE,
                },
                border: Border {
                    radius: (CIRCLE_SIZE / 2.0).into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(accent_color),
        );

        // 3. Draw hover effect for non-selected icon
        let show_hover_a = hover_a && self.is_b_selected;
        let show_hover_b = hover_b && !self.is_b_selected;

        if show_hover_a || show_hover_b {
            let (hover_center_x, hover_center_y) = if show_hover_a {
                (icon_a_center_x, icon_a_center_y)
            } else {
                (icon_b_center_x, icon_b_center_y)
            };
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: hover_center_x - CIRCLE_SIZE / 2.0,
                        y: hover_center_y - CIRCLE_SIZE / 2.0,
                        width: CIRCLE_SIZE,
                        height: CIRCLE_SIZE,
                    },
                    border: Border {
                        radius: (CIRCLE_SIZE / 2.0).into(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                Background::Color(hover_color),
            );
        }

        // 4. Draw icons - color based on proximity to selection circle
        // When animating, both icons should transition their colors smoothly
        let icon_a_selected = percent < 0.5;
        let icon_b_selected = percent >= 0.5;

        let selected_icon_class =
            cosmic::theme::Svg::Custom(Rc::new(move |_theme| cosmic::iced_widget::svg::Style {
                color: Some(Color::WHITE),
            }));

        // Draw icon A
        let icon_a_class = if icon_a_selected {
            selected_icon_class.clone()
        } else {
            cosmic::theme::Svg::Default
        };

        let icon_a_widget =
            icon::Icon::from(icon::from_name(self.icon_a_name).size(ICON_SIZE as u16))
                .width(Length::Fixed(ICON_SIZE))
                .height(Length::Fixed(ICON_SIZE))
                .class(icon_a_class);

        let icon_a_layout =
            layout::Node::new(Size::new(ICON_SIZE, ICON_SIZE)).move_to(cosmic::iced::Point::new(
                icon_a_center_x - ICON_SIZE / 2.0,
                icon_a_center_y - ICON_SIZE / 2.0,
            ));

        // Convert Icon to Element, then get the widget to draw
        let icon_a_element: Element<'_, Msg> = icon_a_widget.into();
        icon_a_element.as_widget().draw(
            &Tree::empty(),
            renderer,
            theme,
            style,
            Layout::new(&icon_a_layout),
            cursor,
            viewport,
        );

        // Draw icon B
        let icon_b_class = if icon_b_selected {
            selected_icon_class
        } else {
            cosmic::theme::Svg::Default
        };

        let icon_b_widget =
            icon::Icon::from(icon::from_name(self.icon_b_name).size(ICON_SIZE as u16))
                .width(Length::Fixed(ICON_SIZE))
                .height(Length::Fixed(ICON_SIZE))
                .class(icon_b_class);

        let icon_b_layout =
            layout::Node::new(Size::new(ICON_SIZE, ICON_SIZE)).move_to(cosmic::iced::Point::new(
                icon_b_center_x - ICON_SIZE / 2.0,
                icon_b_center_y - ICON_SIZE / 2.0,
            ));

        let icon_b_element: Element<'_, Msg> = icon_b_widget.into();
        icon_b_element.as_widget().draw(
            &Tree::empty(),
            renderer,
            theme,
            style,
            Layout::new(&icon_b_layout),
            cursor,
            viewport,
        );
    }

    fn on_event(
        &mut self,
        _tree: &mut Tree,
        event: cosmic::iced_core::Event,
        layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &cosmic::Renderer,
        _clipboard: &mut dyn cosmic::iced_core::Clipboard,
        shell: &mut cosmic::iced_core::Shell<'_, Msg>,
        _viewport: &Rectangle,
    ) -> cosmic::iced_core::event::Status {
        // Only handle events if we have a callback
        let Some(ref on_toggle) = self.on_toggle else {
            return cosmic::iced_core::event::Status::Ignored;
        };

        if let cosmic::iced_core::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) =
            event
        {
            if let Some(pos) = cursor.position() {
                let bounds = layout.bounds();
                let icon_a_bounds = self.icon_a_bounds(bounds);
                let icon_b_bounds = self.icon_b_bounds(bounds);

                if icon_a_bounds.contains(pos) && self.is_b_selected {
                    shell.publish(on_toggle(false));
                    return cosmic::iced_core::event::Status::Captured;
                } else if icon_b_bounds.contains(pos) && !self.is_b_selected {
                    shell.publish(on_toggle(true));
                    return cosmic::iced_core::event::Status::Captured;
                }
            }
        }

        cosmic::iced_core::event::Status::Ignored
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        _viewport: &Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        if let Some(pos) = cursor.position() {
            let bounds = layout.bounds();
            let icon_a_bounds = self.icon_a_bounds(bounds);
            let icon_b_bounds = self.icon_b_bounds(bounds);

            // Show pointer when hovering over the non-selected icon
            if (icon_a_bounds.contains(pos) && self.is_b_selected)
                || (icon_b_bounds.contains(pos) && !self.is_b_selected)
            {
                return mouse::Interaction::Pointer;
            }
        }

        mouse::Interaction::default()
    }

    fn operate(
        &self,
        _tree: &mut Tree,
        _layout: Layout<'_>,
        _renderer: &cosmic::Renderer,
        _operation: &mut dyn cosmic::iced_core::widget::Operation<()>,
    ) {
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut Tree,
        _layout: Layout<'_>,
        _renderer: &cosmic::Renderer,
        _translation: cosmic::iced::Vector,
    ) -> Option<cosmic::iced_core::overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>> {
        None
    }
}

impl<'a, Msg: Clone + 'a> From<IconToggle<'a, Msg>> for Element<'a, Msg> {
    fn from(toggle: IconToggle<'a, Msg>) -> Self {
        Element::new(toggle)
    }
}

/// Creates an icon toggle widget with a pill background and selection circle.
///
/// Visual design (horizontal):
/// ```text
/// +------------------------+
/// |  (A)            B      |  <- A selected
/// +------------------------+
/// ```
///
/// Visual design (vertical):
/// ```text
/// +------+
/// | (A)  |  <- A selected
/// |      |
/// |  B   |
/// +------+
/// ```
///
/// # Example
/// ```ignore
/// // Horizontal toggle with callback
/// icon_toggle(
///     "camera-photo-symbolic",
///     "camera-video-symbolic",
///     is_video_mode,
/// ).on_toggle(Message::ToggleMode)
///
/// // Animated toggle with cosmic-time
/// icon_toggle(
///     "camera-photo-symbolic",
///     "camera-video-symbolic",
///     is_video_mode,
/// )
/// .percent(animation_percent) // 0.0 to 1.0
/// .on_toggle(Message::ToggleMode)
/// ```
pub fn icon_toggle<'a, Msg: Clone + 'a>(
    icon_a: &'a str,
    icon_b: &'a str,
    is_b_selected: bool,
) -> IconToggle<'a, Msg> {
    IconToggle::new(icon_a, icon_b, is_b_selected)
}

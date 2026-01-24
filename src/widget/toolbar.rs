//! Toolbar widget for screenshot actions

use std::rc::Rc;

use cosmic::iced::Length;
use cosmic::iced_core::{layout, widget::Tree, Background, Border, Color, Layout, Size};
use cosmic::iced_renderer::geometry::Renderer as GeometryRenderer;
use cosmic::iced_widget::{canvas, column, container, row};
use cosmic::widget::{button, icon, tooltip};
use cosmic::Element;
use cosmic_time::once_cell::sync::Lazy;
use cosmic_time::{Duration, Ease, Exponential, Timeline, chain, lazy, toggler};

/// Animation ID for toolbar hover opacity
pub static TOOLBAR_HOVER_ID: Lazy<cosmic_time::id::Toggler> =
    Lazy::new(cosmic_time::id::Toggler::unique);

/// Animation duration for toolbar fade in milliseconds
const TOOLBAR_FADE_DURATION_MS: u64 = 200;

/// Get the current toolbar opacity from the timeline
/// Returns 1.0 when hovered (faded in), base_opacity when not hovered (faded out)
pub fn get_toolbar_opacity(timeline: &Timeline, base_opacity: f32, is_hovered: bool) -> f32 {
    let anim_value = timeline
        .get(&TOOLBAR_HOVER_ID.clone().into(), 0)
        .map_or(if is_hovered { 1.0 } else { 0.0 }, |interped| {
            interped.value
        });
    // Interpolate between base_opacity and 1.0
    base_opacity + (1.0 - base_opacity) * anim_value
}

/// Create an animation chain for fading in (unhovered -> hovered)
pub fn toolbar_fade_in() -> cosmic_time::chain::Toggler {
    chain!(
        TOOLBAR_HOVER_ID.clone(),
        lazy::toggler(Duration::ZERO),
        toggler(Duration::from_millis(TOOLBAR_FADE_DURATION_MS))
            .percent(1.0)
            .ease(Ease::Exponential(Exponential::Out)),
    )
}

/// Create an animation chain for fading out (hovered -> unhovered)
pub fn toolbar_fade_out() -> cosmic_time::chain::Toggler {
    chain!(
        TOOLBAR_HOVER_ID.clone(),
        lazy::toggler(Duration::ZERO),
        toggler(Duration::from_millis(TOOLBAR_FADE_DURATION_MS))
            .percent(0.0)
            .ease(Ease::Exponential(Exponential::In)),
    )
}

use super::icon_toggle::icon_toggle;
use super::tool_button::{build_shape_button, build_tool_button};
use super::toolbar_position_selector::ToolbarPositionSelector;
use crate::capture::qr::DetectedQrCode;
use crate::config::{RedactTool, ShapeTool, ToolbarPosition};
use crate::domain::{Choice, DragState, Rect};

/// A wrapper widget that reduces opacity when not hovered
/// Draws a background with opacity and passes through all events
/// Used by both toolbar and settings drawer for consistent appearance
pub struct HoverOpacity<'a, Msg> {
    content: Element<'a, Msg>,
    unhovered_opacity: f32,
    /// When true, always use full opacity (ignores hover state)
    force_opaque: bool,
    /// Callback when hover state changes
    on_hover_change: Option<Box<dyn Fn(bool) -> Msg + 'a>>,
    /// Externally provided content opacity (for animated fading of icons/buttons)
    content_opacity: Option<f32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HatPlacement {
    HeaderTop,
    HeaderBottom,
    HeaderLeft,
    HeaderRight,
}

/// A wrapper widget that renders a connected header + body "hat" background.
pub struct HatContainer<'a, Msg> {
    header: Element<'a, Msg>,
    body: Element<'a, Msg>,
    placement: HatPlacement,
    unhovered_opacity: f32,
    force_opaque: bool,
    /// Callback when hover state changes
    on_hover_change: Option<Box<dyn Fn(bool) -> Msg + 'a>>,
    /// Externally provided content opacity (for animated fading of icons/buttons)
    content_opacity: Option<f32>,
}

const HAT_HIT_RADIUS_FALLBACK: f32 = 8.0;

/// State for HoverOpacity widget to track previous hover state
#[derive(Debug, Clone, Default)]
struct HoverState {
    was_hovered: bool,
}

fn rounded_rect_contains(
    rect: cosmic::iced_core::Rectangle,
    radii: [f32; 4],
    point: cosmic::iced_core::Point,
) -> bool {
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;

    if point.x < rect.x || point.x > right || point.y < rect.y || point.y > bottom {
        return false;
    }

    let corners = [
        (rect.x, rect.y, radii[0]),
        (right, rect.y, radii[1]),
        (right, bottom, radii[2]),
        (rect.x, bottom, radii[3]),
    ];

    for (cx, cy, radius) in corners {
        if radius <= 0.0 {
            continue;
        }

        let (corner_x, corner_y) = match (cx == rect.x, cy == rect.y) {
            (true, true) => (rect.x + radius, rect.y + radius),
            (false, true) => (right - radius, rect.y + radius),
            (false, false) => (right - radius, bottom - radius),
            (true, false) => (rect.x + radius, bottom - radius),
        };

        let within_corner_x = if cx == rect.x {
            point.x < rect.x + radius
        } else {
            point.x > right - radius
        };
        let within_corner_y = if cy == rect.y {
            point.y < rect.y + radius
        } else {
            point.y > bottom - radius
        };

        if within_corner_x && within_corner_y {
            let dx = point.x - corner_x;
            let dy = point.y - corner_y;
            if dx * dx + dy * dy > radius * radius {
                return false;
            }
        }
    }

    true
}

fn hat_contains(
    placement: HatPlacement,
    header_bounds: cosmic::iced_core::Rectangle,
    body_bounds: cosmic::iced_core::Rectangle,
    radius: f32,
    point: cosmic::iced_core::Point,
) -> bool {
    let body_radius: [f32; 4] = match placement {
        HatPlacement::HeaderTop => [0.0, 0.0, radius, radius],
        HatPlacement::HeaderBottom => [radius, radius, 0.0, 0.0],
        HatPlacement::HeaderLeft => [0.0, radius, radius, 0.0],
        HatPlacement::HeaderRight => [radius, 0.0, 0.0, radius],
    };

    let header_radius: [f32; 4] = match placement {
        HatPlacement::HeaderTop => [radius, radius, 0.0, 0.0],
        HatPlacement::HeaderBottom => [0.0, 0.0, radius, radius],
        HatPlacement::HeaderLeft => [radius, 0.0, 0.0, radius],
        HatPlacement::HeaderRight => [0.0, radius, radius, 0.0],
    };

    if rounded_rect_contains(body_bounds, body_radius, point)
        || rounded_rect_contains(header_bounds, header_radius, point)
    {
        return true;
    }

    if radius <= 0.0 {
        return false;
    }

    let centers = match placement {
        HatPlacement::HeaderTop => [
            cosmic::iced_core::Point::new(header_bounds.x, header_bounds.y + header_bounds.height),
            cosmic::iced_core::Point::new(
                header_bounds.x + header_bounds.width,
                header_bounds.y + header_bounds.height,
            ),
        ],
        HatPlacement::HeaderBottom => [
            cosmic::iced_core::Point::new(header_bounds.x, header_bounds.y),
            cosmic::iced_core::Point::new(header_bounds.x + header_bounds.width, header_bounds.y),
        ],
        HatPlacement::HeaderLeft => [
            cosmic::iced_core::Point::new(header_bounds.x + header_bounds.width, header_bounds.y),
            cosmic::iced_core::Point::new(
                header_bounds.x + header_bounds.width,
                header_bounds.y + header_bounds.height,
            ),
        ],
        HatPlacement::HeaderRight => [
            cosmic::iced_core::Point::new(header_bounds.x, header_bounds.y),
            cosmic::iced_core::Point::new(header_bounds.x, header_bounds.y + header_bounds.height),
        ],
    };

    centers.iter().any(|center| {
        let dx = point.x - center.x;
        let dy = point.y - center.y;
        dx * dx + dy * dy <= radius * radius
    })
}

fn build_hat_path(
    placement: HatPlacement,
    header_bounds: cosmic::iced_core::Rectangle,
    body_bounds: cosmic::iced_core::Rectangle,
    radius: f32,
) -> canvas::Path {
    let r = radius
        .min(header_bounds.width / 2.0)
        .min(header_bounds.height / 2.0)
        .min(body_bounds.width / 2.0)
        .min(body_bounds.height / 2.0);

    let h_left = header_bounds.x;
    let h_right = header_bounds.x + header_bounds.width;
    let h_top = header_bounds.y;
    let h_bottom = header_bounds.y + header_bounds.height;

    let b_left = body_bounds.x;
    let b_right = body_bounds.x + body_bounds.width;
    let b_top = body_bounds.y;
    let b_bottom = body_bounds.y + body_bounds.height;

    canvas::Path::new(|builder| match placement {
        HatPlacement::HeaderTop => {
            builder.move_to(cosmic::iced_core::Point::new(h_left + r, h_top));
            builder.line_to(cosmic::iced_core::Point::new(h_right - r, h_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_right, h_top),
                cosmic::iced_core::Point::new(h_right, h_top + r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_right, h_bottom - r));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_right, b_top),
                cosmic::iced_core::Point::new(h_right + r, b_top),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_right - r, b_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, b_top),
                cosmic::iced_core::Point::new(b_right, b_top + r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_right, b_bottom - r));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, b_bottom),
                cosmic::iced_core::Point::new(b_right - r, b_bottom),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left + r, b_bottom));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, b_bottom),
                cosmic::iced_core::Point::new(b_left, b_bottom - r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left, b_top + r));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, b_top),
                cosmic::iced_core::Point::new(b_left + r, b_top),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_left - r, b_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_left, b_top),
                cosmic::iced_core::Point::new(h_left, b_top - r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_left, h_top + r));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_left, h_top),
                cosmic::iced_core::Point::new(h_left + r, h_top),
                r,
            );
            builder.close();
        }
        HatPlacement::HeaderBottom => {
            builder.move_to(cosmic::iced_core::Point::new(b_left + r, b_top));
            builder.line_to(cosmic::iced_core::Point::new(b_right - r, b_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, b_top),
                cosmic::iced_core::Point::new(b_right, b_top + r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_right, b_bottom - r));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, b_bottom),
                cosmic::iced_core::Point::new(b_right - r, b_bottom),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_right + r, b_bottom));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_right, b_bottom),
                cosmic::iced_core::Point::new(h_right, b_bottom + r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_right, h_bottom - r));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_right, h_bottom),
                cosmic::iced_core::Point::new(h_right - r, h_bottom),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_left + r, h_bottom));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_left, h_bottom),
                cosmic::iced_core::Point::new(h_left, h_bottom - r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_left, h_top + r));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_left, h_top),
                cosmic::iced_core::Point::new(h_left - r, h_top),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left + r, h_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, h_top),
                cosmic::iced_core::Point::new(b_left, h_top - r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left, b_top + r));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, b_top),
                cosmic::iced_core::Point::new(b_left + r, b_top),
                r,
            );
            builder.close();
        }
        HatPlacement::HeaderLeft => {
            builder.move_to(cosmic::iced_core::Point::new(b_left + r, b_top));
            builder.line_to(cosmic::iced_core::Point::new(b_right - r, b_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, b_top),
                cosmic::iced_core::Point::new(b_right, b_top + r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_right, b_bottom - r));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, b_bottom),
                cosmic::iced_core::Point::new(b_right - r, b_bottom),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left + r, b_bottom));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, b_bottom),
                cosmic::iced_core::Point::new(b_left, b_bottom - r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left, h_bottom + r));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, h_bottom),
                cosmic::iced_core::Point::new(b_left - r, h_bottom),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_left + r, h_bottom));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_left, h_bottom),
                cosmic::iced_core::Point::new(h_left, h_bottom - r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_left, h_top + r));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_left, h_top),
                cosmic::iced_core::Point::new(h_left + r, h_top),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left - r, h_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, h_top),
                cosmic::iced_core::Point::new(b_left, h_top - r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left + r, b_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, b_top),
                cosmic::iced_core::Point::new(b_left + r, b_top),
                r,
            );
            builder.close();
        }
        HatPlacement::HeaderRight => {
            builder.move_to(cosmic::iced_core::Point::new(b_left + r, b_top));
            builder.line_to(cosmic::iced_core::Point::new(b_right - r, b_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, b_top),
                cosmic::iced_core::Point::new(b_right, b_top + r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_right, h_top - r));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, h_top),
                cosmic::iced_core::Point::new(b_right + r, h_top),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_right - r, h_top));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_right, h_top),
                cosmic::iced_core::Point::new(h_right, h_top + r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(h_right, h_bottom - r));
            builder.arc_to(
                cosmic::iced_core::Point::new(h_right, h_bottom),
                cosmic::iced_core::Point::new(h_right - r, h_bottom),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_right + r, h_bottom));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, h_bottom),
                cosmic::iced_core::Point::new(b_right, h_bottom + r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_right, b_bottom - r));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_right, b_bottom),
                cosmic::iced_core::Point::new(b_right - r, b_bottom),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left + r, b_bottom));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, b_bottom),
                cosmic::iced_core::Point::new(b_left, b_bottom - r),
                r,
            );
            builder.line_to(cosmic::iced_core::Point::new(b_left, b_top + r));
            builder.arc_to(
                cosmic::iced_core::Point::new(b_left, b_top),
                cosmic::iced_core::Point::new(b_left + r, b_top),
                r,
            );
            builder.close();
        }
    })
}

impl<'a, Msg: 'static + Clone> HoverOpacity<'a, Msg> {
    pub fn new(content: impl Into<Element<'a, Msg>>) -> Self {
        Self {
            content: content.into(),
            unhovered_opacity: 0.5,
            force_opaque: false,
            on_hover_change: None,
            content_opacity: None,
        }
    }

    /// Set opacity when not hovered (0.0 to 1.0)
    pub fn unhovered_opacity(mut self, opacity: f32) -> Self {
        self.unhovered_opacity = opacity;
        self
    }

    /// Force full opacity regardless of hover state
    pub fn force_opaque(mut self, force: bool) -> Self {
        self.force_opaque = force;
        self
    }

    /// Set callback for hover state changes
    pub fn on_hover_change(mut self, callback: impl Fn(bool) -> Msg + 'a) -> Self {
        self.on_hover_change = Some(Box::new(callback));
        self
    }

    /// Set content opacity (for animated fading of icons/buttons)
    pub fn content_opacity(mut self, opacity: f32) -> Self {
        self.content_opacity = Some(opacity);
        self
    }
}

impl<'a, Msg: 'static + Clone> HatContainer<'a, Msg> {
    pub fn new(header: impl Into<Element<'a, Msg>>, body: impl Into<Element<'a, Msg>>) -> Self {
        Self {
            header: header.into(),
            body: body.into(),
            placement: HatPlacement::HeaderTop,
            unhovered_opacity: 0.5,
            force_opaque: false,
            on_hover_change: None,
            content_opacity: None,
        }
    }

    pub fn placement(mut self, placement: HatPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn force_opaque(mut self, force: bool) -> Self {
        self.force_opaque = force;
        self
    }

    pub fn unhovered_opacity(mut self, opacity: f32) -> Self {
        self.unhovered_opacity = opacity.clamp(0.1, 1.0);
        self
    }

    /// Set callback for hover state changes
    pub fn on_hover_change(mut self, callback: impl Fn(bool) -> Msg + 'a) -> Self {
        self.on_hover_change = Some(Box::new(callback));
        self
    }

    /// Set content opacity (for animated fading of icons/buttons)
    pub fn content_opacity(mut self, opacity: f32) -> Self {
        self.content_opacity = Some(opacity);
        self
    }
}

impl<'a, Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for HoverOpacity<'a, Msg>
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn state(&self) -> cosmic::iced_core::widget::tree::State {
        cosmic::iced_core::widget::tree::State::new(HoverState::default())
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &cosmic::iced::Limits,
    ) -> layout::Node {
        self.content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
    ) {
        use cosmic::iced_core::Renderer as _;

        let bounds = layout.bounds();
        let is_hovered = cursor
            .position()
            .map(|p| bounds.contains(p))
            .unwrap_or(false);
        let opacity = if self.force_opaque || is_hovered {
            1.0
        } else {
            self.unhovered_opacity
        };

        let cosmic_theme = theme.cosmic();
        let radius = cosmic_theme.corner_radii.radius_s;

        // Draw the background with appropriate opacity
        let mut bg_color: cosmic::iced::Color = cosmic_theme.background.component.base.into();
        bg_color.a *= opacity;

        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds,
                border: Border {
                    radius: radius.into(),
                    ..Default::default()
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(bg_color),
        );

        // Apply opacity to the text color style
        let mut draw_style = *style;
        draw_style.text_color.a *= opacity;

        // Draw content
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            &draw_style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn cosmic::iced_core::widget::Operation<()>,
    ) {
        self.content
            .as_widget()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: cosmic::iced_core::Event,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn cosmic::iced_core::Clipboard,
        shell: &mut cosmic::iced_core::Shell<'_, Msg>,
        viewport: &cosmic::iced_core::Rectangle,
    ) -> cosmic::iced_core::event::Status {
        // Check for hover state changes on any mouse event
        if let cosmic::iced_core::Event::Mouse(_) = &event {
            if let Some(ref on_hover_change) = self.on_hover_change {
                let bounds = layout.bounds();
                let is_hovered = cursor
                    .position()
                    .map(|p| bounds.contains(p))
                    .unwrap_or(false);

                let state = tree.state.downcast_mut::<HoverState>();
                if state.was_hovered != is_hovered {
                    state.was_hovered = is_hovered;
                    shell.publish(on_hover_change(is_hovered));
                }
            }
        }

        self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
        renderer: &cosmic::Renderer,
    ) -> cosmic::iced_core::mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        translation: cosmic::iced::Vector,
    ) -> Option<cosmic::iced_core::overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, translation)
    }
}

impl<'a, Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for HatContainer<'a, Msg>
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn state(&self) -> cosmic::iced_core::widget::tree::State {
        cosmic::iced_core::widget::tree::State::new(HoverState::default())
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &cosmic::iced::Limits,
    ) -> layout::Node {
        let header_node = self
            .header
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits);
        let body_node = self
            .body
            .as_widget()
            .layout(&mut tree.children[1], renderer, limits);

        let header_bounds = header_node.bounds();
        let body_bounds = body_node.bounds();

        let (width, height) = match self.placement {
            HatPlacement::HeaderTop | HatPlacement::HeaderBottom => (
                header_bounds.width.max(body_bounds.width),
                header_bounds.height + body_bounds.height,
            ),
            HatPlacement::HeaderLeft | HatPlacement::HeaderRight => (
                header_bounds.width + body_bounds.width,
                header_bounds.height.max(body_bounds.height),
            ),
        };

        let (header_pos, body_pos) = match self.placement {
            HatPlacement::HeaderTop => (
                cosmic::iced::Point::new((width - header_bounds.width) / 2.0, 0.0),
                cosmic::iced::Point::new((width - body_bounds.width) / 2.0, header_bounds.height),
            ),
            HatPlacement::HeaderBottom => (
                cosmic::iced::Point::new((width - header_bounds.width) / 2.0, body_bounds.height),
                cosmic::iced::Point::new((width - body_bounds.width) / 2.0, 0.0),
            ),
            HatPlacement::HeaderLeft => (
                cosmic::iced::Point::new(0.0, (height - header_bounds.height) / 2.0),
                cosmic::iced::Point::new(header_bounds.width, (height - body_bounds.height) / 2.0),
            ),
            HatPlacement::HeaderRight => (
                cosmic::iced::Point::new(body_bounds.width, (height - header_bounds.height) / 2.0),
                cosmic::iced::Point::new(0.0, (height - body_bounds.height) / 2.0),
            ),
        };

        let header_node = header_node.move_to(header_pos);
        let body_node = body_node.move_to(body_pos);

        layout::Node::with_children(Size::new(width, height), vec![header_node, body_node])
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.header), Tree::new(&self.body)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        let mut children = [&mut self.header, &mut self.body];
        tree.diff_children(&mut children);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced_core::renderer::Style,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
    ) {
        use cosmic::iced_core::Renderer as _;

        let cosmic_theme = theme.cosmic();
        let radius = cosmic_theme.corner_radii.radius_s[0];

        let mut children = layout.children();
        let header_layout = children.next();
        let body_layout = children.next();

        let header_bounds = header_layout.as_ref().map(|layout| layout.bounds());
        let body_bounds = body_layout.as_ref().map(|layout| layout.bounds());

        let is_hovered = if let (Some(header_bounds), Some(body_bounds), Some(position)) =
            (header_bounds, body_bounds, cursor.position())
        {
            hat_contains(self.placement, header_bounds, body_bounds, radius, position)
        } else {
            false
        };
        let opacity = if self.force_opaque || is_hovered {
            1.0
        } else {
            self.unhovered_opacity
        };

        let mut bg_color: cosmic::iced::Color = cosmic_theme.background.component.base.into();
        bg_color.a *= opacity;

        let mut draw_style = *style;
        draw_style.text_color.a *= opacity;

        if let (Some(header_bounds), Some(body_bounds)) = (header_bounds, body_bounds) {
            let bounds = layout.bounds();
            let header_bounds = cosmic::iced_core::Rectangle {
                x: header_bounds.x - bounds.x,
                y: header_bounds.y - bounds.y,
                width: header_bounds.width,
                height: header_bounds.height,
            };
            let body_bounds = cosmic::iced_core::Rectangle {
                x: body_bounds.x - bounds.x,
                y: body_bounds.y - bounds.y,
                width: body_bounds.width,
                height: body_bounds.height,
            };
            let hat_path = build_hat_path(self.placement, header_bounds, body_bounds, radius);

            renderer.with_translation(cosmic::iced::Vector::new(bounds.x, bounds.y), |renderer| {
                let mut frame = canvas::Frame::new(renderer, bounds.size());
                frame.fill(&hat_path, bg_color);
                renderer.draw_geometry(frame.into_geometry());
            });
        }

        renderer.with_layer(layout.bounds(), |renderer| {
            if let Some(header_layout) = header_layout {
                self.header.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    &draw_style,
                    header_layout,
                    cursor,
                    viewport,
                );
            }

            if let Some(body_layout) = body_layout {
                self.body.as_widget().draw(
                    &tree.children[1],
                    renderer,
                    theme,
                    &draw_style,
                    body_layout,
                    cursor,
                    viewport,
                );
            }
        });
    }

    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn cosmic::iced_core::widget::Operation<()>,
    ) {
        let mut children = layout.children();
        let header_layout = children.next();
        let body_layout = children.next();

        if let Some(header_layout) = header_layout {
            self.header.as_widget().operate(
                &mut tree.children[0],
                header_layout,
                renderer,
                operation,
            );
        }

        if let Some(body_layout) = body_layout {
            self.body
                .as_widget()
                .operate(&mut tree.children[1], body_layout, renderer, operation);
        }
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: cosmic::iced_core::Event,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn cosmic::iced_core::Clipboard,
        shell: &mut cosmic::iced_core::Shell<'_, Msg>,
        viewport: &cosmic::iced_core::Rectangle,
    ) -> cosmic::iced_core::event::Status {
        let mut children = layout.children();
        let header_layout = children.next();
        let body_layout = children.next();

        // Check for hover state changes on any mouse event
        if matches!(event, cosmic::iced_core::Event::Mouse(_)) {
            if let (Some(header_layout), Some(body_layout)) = (header_layout, body_layout) {
                let header_bounds = header_layout.bounds();
                let body_bounds = body_layout.bounds();
                let radius = HAT_HIT_RADIUS_FALLBACK;

                let is_hovered = cursor
                    .position()
                    .map(|p| hat_contains(self.placement, header_bounds, body_bounds, radius, p))
                    .unwrap_or(false);

                // Emit hover change callback
                if let Some(ref on_hover_change) = self.on_hover_change {
                    let state = tree.state.downcast_mut::<HoverState>();
                    if state.was_hovered != is_hovered {
                        state.was_hovered = is_hovered;
                        shell.publish(on_hover_change(is_hovered));
                    }
                }

                // Don't block events when not hovered - allow clicks through
                // The hover state is only for visual feedback (opacity animation)
                // if !is_hovered {
                //     return cosmic::iced_core::event::Status::Ignored;
                // }
            }
        }

        let mut children = layout.children();
        let header_layout = children.next();
        let body_layout = children.next();

        if let Some(header_layout) = header_layout {
            let status = self.header.as_widget_mut().on_event(
                &mut tree.children[0],
                event.clone(),
                header_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );

            if status == cosmic::iced_core::event::Status::Captured {
                return status;
            }
        }

        if let Some(body_layout) = body_layout {
            return self.body.as_widget_mut().on_event(
                &mut tree.children[1],
                event,
                body_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }

        cosmic::iced_core::event::Status::Ignored
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: cosmic::iced_core::mouse::Cursor,
        viewport: &cosmic::iced_core::Rectangle,
        renderer: &cosmic::Renderer,
    ) -> cosmic::iced_core::mouse::Interaction {
        let mut children = layout.children();
        let header_layout = children.next();
        let body_layout = children.next();

        if let (Some(header_layout), Some(body_layout), Some(position)) =
            (header_layout, body_layout, cursor.position())
        {
            let radius = HAT_HIT_RADIUS_FALLBACK;
            if !hat_contains(
                self.placement,
                header_layout.bounds(),
                body_layout.bounds(),
                radius,
                position,
            ) {
                return cosmic::iced_core::mouse::Interaction::Idle;
            }
        }

        let header_interaction = header_layout.map_or(
            cosmic::iced_core::mouse::Interaction::Idle,
            |header_layout| {
                self.header.as_widget().mouse_interaction(
                    &tree.children[0],
                    header_layout,
                    cursor,
                    viewport,
                    renderer,
                )
            },
        );
        let body_interaction =
            body_layout.map_or(cosmic::iced_core::mouse::Interaction::Idle, |body_layout| {
                self.body.as_widget().mouse_interaction(
                    &tree.children[1],
                    body_layout,
                    cursor,
                    viewport,
                    renderer,
                )
            });

        header_interaction.max(body_interaction)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        translation: cosmic::iced::Vector,
    ) -> Option<cosmic::iced_core::overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>> {
        let (header_tree, body_tree) = tree.children.split_at_mut(1);
        let header_overlay =
            self.header
                .as_widget_mut()
                .overlay(&mut header_tree[0], layout, renderer, translation);
        if header_overlay.is_some() {
            return header_overlay;
        }

        self.body
            .as_widget_mut()
            .overlay(&mut body_tree[0], layout, renderer, translation)
    }
}

impl<'a, Msg: Clone + 'static> From<HatContainer<'a, Msg>> for Element<'a, Msg> {
    fn from(widget: HatContainer<'a, Msg>) -> Self {
        Element::new(widget)
    }
}

impl<'a, Msg: Clone + 'static> From<HoverOpacity<'a, Msg>> for Element<'a, Msg> {
    fn from(widget: HoverOpacity<'a, Msg>) -> Self {
        Element::new(widget)
    }
}

/// Build the screenshot toolbar element
#[allow(clippy::too_many_arguments)]
pub fn build_toolbar<'a, Msg: Clone + 'static>(
    choice: Choice,
    output_name: String,
    toolbar_position: ToolbarPosition,
    has_selection: bool,
    has_ocr_text: bool,
    qr_codes: &[DetectedQrCode],
    primary_shape_tool: ShapeTool,
    shape_mode_active: bool,
    shape_popup_open: bool,
    primary_redact_tool: RedactTool,
    redact_mode_active: bool,
    redact_popup_open: bool,
    space_s: u16,
    space_xs: u16,
    space_xxs: u16,
    on_choice_change: impl Fn(Choice) -> Msg + 'static + Clone,
    on_copy_to_clipboard: Msg,
    on_save_to_pictures: Msg,
    on_record_region: Msg,
    on_stop_recording: Msg,
    on_toggle_recording_annotation: Msg,
    on_shape_press: Msg,
    on_shape_right_click: Msg,
    on_redact_press: Msg,
    on_redact_right_click: Msg,
    on_ocr: Msg,
    on_ocr_copy: Msg,
    on_qr: Msg,
    on_qr_copy: Msg,
    on_cancel: Msg,
    on_toolbar_position: &(impl Fn(ToolbarPosition) -> Msg + 'a),
    on_settings_toggle: Msg,
    settings_drawer_open: bool,
    force_toolbar_opaque: bool,
    toolbar_unhovered_opacity: f32,
    output_count: usize,
    tesseract_available: bool,
    is_video_mode: bool,
    is_recording: bool,
    recording_annotation_mode: bool,
    toggle_animation_percent: f32,
    on_capture_mode_toggle: impl Fn(bool) -> Msg + 'a,
    content_opacity: f32,
    on_hover_change: impl Fn(bool) -> Msg + 'a,
) -> Element<'a, Msg> {
    use cosmic::widget::divider::vertical;

    let is_vertical = matches!(
        toolbar_position,
        ToolbarPosition::Left | ToolbarPosition::Right
    );

    // Helper: SVG style with opacity for active (accent colored) icons
    let active_icon = {
        let opacity = content_opacity;
        cosmic::theme::Svg::Custom(Rc::new(move |theme| {
            let mut color: Color = theme.cosmic().accent_color().into();
            color.a *= opacity;
            cosmic::iced_widget::svg::Style { color: Some(color) }
        }))
    };

    // Helper: SVG style with opacity for default (theme colored) icons
    let default_icon = {
        let opacity = content_opacity;
        cosmic::theme::Svg::Custom(Rc::new(move |theme| {
            let mut color: Color = theme.cosmic().background.component.on.into();
            color.a *= opacity;
            cosmic::iced_widget::svg::Style { color: Some(color) }
        }))
    };

    // Position selector - custom widget with triangular hit regions
    let position_selector: Element<'_, Msg> = tooltip(
        ToolbarPositionSelector::new(
            40.0, // size of the selector widget
            toolbar_position,
            on_toolbar_position(ToolbarPosition::Top),
            on_toolbar_position(ToolbarPosition::Bottom),
            on_toolbar_position(ToolbarPosition::Left),
            on_toolbar_position(ToolbarPosition::Right),
        )
        .opacity(content_opacity),
        "Move Toolbar (Ctrl+hjkl)",
        tooltip::Position::Bottom,
    )
    .into();

    // Mode toggle - switch between screenshot and video recording (animated)
    let toggle_widget = icon_toggle(
        "camera-photo-symbolic",
        "camera-video-symbolic",
        is_video_mode,
    )
    .percent(toggle_animation_percent)
    .opacity(content_opacity)
    .on_toggle(on_capture_mode_toggle);
    let mode_toggle: Element<'_, Msg> = tooltip(
        if is_vertical {
            toggle_widget.vertical()
        } else {
            toggle_widget
        },
        "Screenshot / Video",
        tooltip::Position::Bottom,
    )
    .into();

    // Common buttons with tooltips
    let btn_region = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("screenshot-selection-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
                .class(if matches!(choice, Choice::Rectangle(..)) {
                    active_icon.clone()
                } else {
                    default_icon.clone()
                }),
        )
        .selected(matches!(choice, Choice::Rectangle(..)))
        .class(cosmic::theme::Button::Icon)
        .on_press(on_choice_change(Choice::Rectangle(
            Rect::default(),
            DragState::None,
        )))
        .padding(space_xs),
        "Select Region (R)",
        tooltip::Position::Bottom,
    );

    let btn_window = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("screenshot-window-symbolic").size(64))
                .class(if matches!(choice, Choice::Window(..)) {
                    active_icon.clone()
                } else {
                    default_icon.clone()
                })
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .selected(matches!(choice, Choice::Window(..)))
        .class(cosmic::theme::Button::Icon)
        .on_press(on_choice_change(Choice::Window(output_name.clone(), None)))
        .padding(space_xs),
        "Select Window (W)",
        tooltip::Position::Bottom,
    );

    let btn_screen = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("screenshot-screen-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
                .class(if matches!(choice, Choice::Output(..)) {
                    active_icon.clone()
                } else {
                    default_icon.clone()
                }),
        )
        .selected(matches!(choice, Choice::Output(..)))
        .class(cosmic::theme::Button::Icon)
        .on_press(on_choice_change(Choice::Output(None))) // Goes to picker mode
        .padding(space_xs),
        "Select Screen (S)",
        tooltip::Position::Bottom,
    );

    // Context-sensitive copy tooltip
    let copy_tooltip = match &choice {
        Choice::Rectangle(r, _) if r.dimensions().is_some() => "Copy Selected Region (Enter)",
        Choice::Window(_, Some(_)) => "Copy Selected Window (Enter)",
        Choice::Output(Some(_)) => "Copy Selected Screen (Enter)",
        _ if output_count > 1 => "Copy All Screens (Enter)",
        _ => "Copy Screen (Enter)",
    };

    // Context-sensitive save tooltip
    let save_tooltip = match &choice {
        Choice::Rectangle(r, _) if r.dimensions().is_some() => "Save Selected Region (Ctrl+Enter)",
        Choice::Window(_, Some(_)) => "Save Selected Window (Ctrl+Enter)",
        Choice::Output(Some(_)) => "Save Selected Screen (Ctrl+Enter)",
        _ if output_count > 1 => "Save All Screens (Ctrl+Enter)",
        _ => "Save Screen (Ctrl+Enter)",
    };

    // Copy to clipboard button - always enabled
    let btn_copy = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .class(cosmic::theme::Button::Icon)
        .on_press(on_copy_to_clipboard)
        .padding(space_xs),
        copy_tooltip,
        tooltip::Position::Bottom,
    );

    // Save to pictures button - always enabled
    let btn_save = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("document-save-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .class(cosmic::theme::Button::Icon)
        .on_press(on_save_to_pictures)
        .padding(space_xs),
        save_tooltip,
        tooltip::Position::Bottom,
    );

    // Record button - enabled only when region is selected
    // Custom red circular button with themed border
    let record_icon = container(
        icon::Icon::from(icon::from_name("media-record-symbolic").size(64))
            .class({
                let opacity = content_opacity;
                cosmic::theme::Svg::Custom(Rc::new(move |_theme| {
                    cosmic::iced_widget::svg::Style {
                        color: Some(Color::from_rgba(1.0, 1.0, 1.0, opacity)),
                    }
                }))
            })
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(24.0)),
    )
    .class({
        let opacity = content_opacity;
        cosmic::theme::Container::Custom(Box::new(move |theme| {
            let cosmic_theme = theme.cosmic();
            // Check if dark theme by examining background luminance
            let bg = cosmic_theme.background.base;
            let is_dark = (bg.red * 0.299 + bg.green * 0.587 + bg.blue * 0.114) < 0.5;
            let border_color = if is_dark {
                Color::from_rgba(1.0, 1.0, 1.0, opacity)
            } else {
                Color::from_rgba(0.0, 0.0, 0.0, opacity)
            };
            let red_color = if has_selection {
                Color::from_rgba(0.85, 0.2, 0.2, opacity) // Bright red when enabled
            } else {
                Color::from_rgba(0.5, 0.3, 0.3, opacity) // Muted red when disabled
            };
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(red_color)),
                border: Border {
                    radius: 20.0.into(), // Circular
                    width: 2.0,
                    color: border_color,
                },
                ..Default::default()
            }
        }))
    })
    .padding(8)
    .width(Length::Fixed(40.0))
    .height(Length::Fixed(40.0))
    .align_x(cosmic::iced_core::alignment::Horizontal::Center)
    .align_y(cosmic::iced_core::alignment::Vertical::Center);

    let record_tooltip = if has_selection {
        "Record selection (Shift+R)"
    } else {
        "Disabled: select a region, window, or screen first"
    };

    let btn_record = tooltip(
        button::custom(record_icon)
            .class(cosmic::theme::Button::Icon)
            .on_press_maybe(has_selection.then_some(on_record_region))
            .padding(0),
        record_tooltip,
        tooltip::Position::Bottom,
    );

    // Stop recording button - square stop icon in red circle
    let stop_icon = container(
        icon::Icon::from(icon::from_name("media-playback-stop-symbolic").size(64))
            .class({
                let opacity = content_opacity;
                cosmic::theme::Svg::Custom(Rc::new(move |_theme| {
                    cosmic::iced_widget::svg::Style {
                        color: Some(Color::from_rgba(1.0, 1.0, 1.0, opacity)),
                    }
                }))
            })
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(24.0)),
    )
    .class({
        let opacity = content_opacity;
        cosmic::theme::Container::Custom(Box::new(move |theme| {
            let cosmic_theme = theme.cosmic();
            let bg = cosmic_theme.background.base;
            let is_dark = (bg.red * 0.299 + bg.green * 0.587 + bg.blue * 0.114) < 0.5;
            let border_color = if is_dark {
                Color::from_rgba(1.0, 1.0, 1.0, opacity)
            } else {
                Color::from_rgba(0.0, 0.0, 0.0, opacity)
            };
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(Color::from_rgba(
                    0.85, 0.2, 0.2, opacity,
                ))),
                border: Border {
                radius: 20.0.into(),
                width: 2.0,
                color: border_color,
            },
            ..Default::default()
        }
    }))
})
    .padding(8)
    .width(Length::Fixed(40.0))
    .height(Length::Fixed(40.0))
    .align_x(cosmic::iced_core::alignment::Horizontal::Center)
    .align_y(cosmic::iced_core::alignment::Vertical::Center);

    let btn_stop_recording = tooltip(
        button::custom(stop_icon)
            .class(cosmic::theme::Button::Icon)
            .on_press(on_stop_recording)
            .padding(0),
        "Stop Recording",
        tooltip::Position::Bottom,
    );

    // Annotation toggle button for recording mode (pencil icon)
    // This toggles freehand drawing mode on the recording overlay
    let btn_recording_annotate = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("edit-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
                .class(if recording_annotation_mode {
                    active_icon.clone()
                } else {
                    default_icon.clone()
                }),
        )
        .selected(recording_annotation_mode)
        .class(cosmic::theme::Button::Icon)
        .on_press(on_toggle_recording_annotation)
        .padding(space_xs),
        "Freehand Annotation",
        tooltip::Position::Bottom,
    );

    // Shape drawing button with indicator dots
    // - Normal click: triggers primary action (toggles mode)
    // - Right-click or long-press: triggers secondary action (opens popup)
    let btn_shapes: Element<'_, Msg> = build_shape_button(
        primary_shape_tool,
        shape_mode_active,
        shape_popup_open,
        has_selection,
        has_selection.then_some(on_shape_press.clone()),
        has_selection.then_some(on_shape_right_click.clone()),
        space_xs,
        space_xxs,
        content_opacity,
    );

    // Redact/Pixelate tool button (combined)
    let btn_redact = build_tool_button(
        primary_redact_tool.icon_name(),
        primary_redact_tool.tooltip(),
        2, // 2 options: Redact and Pixelate
        primary_redact_tool.index(),
        redact_mode_active,
        redact_popup_open,
        has_selection,
        has_selection.then_some(on_redact_press.clone()),
        has_selection.then_some(on_redact_right_click.clone()),
        space_xs,
        content_opacity,
    );

    // OCR button
    let btn_ocr = if has_ocr_text {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Suggested)
            .on_press_maybe(has_selection.then_some(on_ocr_copy.clone()))
            .padding(space_xs),
            "Copy OCR Text (O)",
            tooltip::Position::Bottom,
        )
    } else if tesseract_available {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("ocr-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Icon)
            .on_press_maybe(has_selection.then_some(on_ocr.clone()))
            .padding(space_xs),
            "Recognize Text (O)",
            tooltip::Position::Bottom,
        )
    } else {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("ocr-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Icon)
            .on_press_maybe(None)
            .padding(space_xs),
            "Install tesseract to enable OCR",
            tooltip::Position::Bottom,
        )
    };

    // QR button
    let has_qr_codes = !qr_codes.is_empty();
    let btn_qr = if has_qr_codes {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Suggested)
            .on_press_maybe(has_selection.then_some(on_qr_copy.clone()))
            .padding(space_xs),
            "Copy QR Code (Q)",
            tooltip::Position::Bottom,
        )
    } else {
        tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("qr-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(cosmic::theme::Button::Icon)
            .on_press_maybe(has_selection.then_some(on_qr.clone()))
            .padding(space_xs),
            "Scan QR Code (Q)",
            tooltip::Position::Bottom,
        )
    };

    // Settings button - responds to both left and right click
    let btn_settings: Element<'_, Msg> = {
        let settings_btn = tooltip(
            button::custom(
                icon::Icon::from(icon::from_name("application-menu-symbolic").size(64))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0)),
            )
            .class(if settings_drawer_open {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Icon
            })
            .on_press(on_settings_toggle.clone())
            .padding(space_xs),
            "Settings",
            tooltip::Position::Bottom,
        );
        super::tool_button::RightClickWrapper::new(settings_btn, Some(on_settings_toggle)).into()
    };

    let btn_close = tooltip(
        button::custom(
            icon::Icon::from(icon::from_name("window-close-symbolic").size(63))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0)),
        )
        .class(cosmic::theme::Button::Icon)
        .on_press(on_cancel),
        "Cancel",
        tooltip::Position::Bottom,
    );

    let toolbar_body_content: Element<'_, Msg> = if is_vertical {
        // Vertical layout for left/right positions
        use cosmic::widget::divider::horizontal;
        if is_recording {
            // Recording mode: only position, annotation toggle, and stop button
            column![
                position_selector,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_recording_annotate]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_stop_recording]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
            ]
            .align_x(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_s, space_xxs, space_s, space_xxs])
            .into()
        } else if is_video_mode {
            column![
                position_selector,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_region, btn_screen]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_record]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
            ]
            .align_x(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_s, space_xxs, space_s, space_xxs])
            .into()
        } else if has_selection {
            let tool_buttons = column![btn_shapes, btn_redact, btn_ocr, btn_qr]
                .spacing(space_s)
                .align_x(cosmic::iced_core::Alignment::Center);

            column![
                position_selector,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_region, btn_window, btn_screen]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                tool_buttons,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_copy, btn_save]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
            ]
            .align_x(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_s, space_xxs, space_s, space_xxs])
            .into()
        } else {
            column![
                position_selector,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_region, btn_window, btn_screen]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_copy, btn_save]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_x(cosmic::iced_core::Alignment::Center),
            ]
            .align_x(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_s, space_xxs, space_s, space_xxs])
            .into()
        }
    } else {
        // Horizontal layout for top/bottom positions
        if is_recording {
            // Recording mode: only position, annotation toggle, and stop button
            row![
                position_selector,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_recording_annotate]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_stop_recording]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
            ]
            .align_y(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_xxs, space_s, space_xxs, space_s])
            .into()
        } else if is_video_mode {
            row![
                position_selector,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_region, btn_screen]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_record]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
            ]
            .align_y(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_xxs, space_s, space_xxs, space_s])
            .into()
        } else if has_selection {
            let tool_buttons = row![btn_shapes, btn_redact, btn_ocr, btn_qr]
                .spacing(space_s)
                .align_y(cosmic::iced_core::Alignment::Center);

            row![
                position_selector,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_region, btn_window, btn_screen]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                tool_buttons,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_copy, btn_save]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
            ]
            .align_y(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_xxs, space_s, space_xxs, space_s])
            .into()
        } else {
            row![
                position_selector,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_region, btn_window, btn_screen]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_copy, btn_save]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_settings, btn_close]
                    .spacing(space_s)
                    .align_y(cosmic::iced_core::Alignment::Center),
            ]
            .align_y(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_xxs, space_s, space_xxs, space_s])
            .into()
        }
    };

    let toolbar_body = cosmic::widget::container(toolbar_body_content).class(
        cosmic::theme::Container::Custom(Box::new(|theme| {
            let theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: None, // HatContainer draws the background with opacity
                text_color: Some(theme.background.component.on.into()),
                border: Border::default(),
                ..Default::default()
            }
        })),
    );

    // When recording, hide the header (mode toggle) - just show body with HoverOpacity
    if is_recording {
        return HoverOpacity::new(toolbar_body)
            .unhovered_opacity(toolbar_unhovered_opacity)
            .force_opaque(force_toolbar_opaque)
            .content_opacity(content_opacity)
            .on_hover_change(on_hover_change)
            .into();
    }

    let toolbar_toggle = cosmic::widget::container(mode_toggle)
        .padding(space_xxs)
        .class(cosmic::theme::Container::Custom(Box::new(|theme| {
            let theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: None, // HatContainer draws the background with opacity
                text_color: Some(theme.background.component.on.into()),
                border: Border::default(),
                ..Default::default()
            }
        })));

    let placement = match toolbar_position {
        ToolbarPosition::Top => HatPlacement::HeaderBottom,
        ToolbarPosition::Bottom => HatPlacement::HeaderTop,
        ToolbarPosition::Left => HatPlacement::HeaderRight,
        ToolbarPosition::Right => HatPlacement::HeaderLeft,
    };

    HatContainer::new(toolbar_toggle, toolbar_body)
        .placement(placement)
        .unhovered_opacity(toolbar_unhovered_opacity)
        .force_opaque(force_toolbar_opaque)
        .content_opacity(content_opacity)
        .on_hover_change(on_hover_change)
        .into()
}

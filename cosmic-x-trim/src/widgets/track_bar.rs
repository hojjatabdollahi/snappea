// SPDX-License-Identifier: GPL-3.0-only

//! A media track with trim handles and a playhead. The caller owns `start`,
//! `end` and `position`. The widget owns only pointer state.

use cosmic::iced::Color;
use cosmic::iced::advanced::graphics::geometry::Renderer as GeometryRenderer;
use cosmic::iced::core::event::Event;
use cosmic::iced::core::mouse::{self, Cursor};
use cosmic::iced::core::renderer::{self, Quad, Renderer as RendererTrait};
use cosmic::iced::core::text::Renderer as TextRenderer;
use cosmic::iced::core::widget::tree::{self, Tree};
use cosmic::iced::core::{
    Background, Border, Element, Layout, Length, Point, Rectangle, Shell, Size, Widget, layout,
    touch,
};
use cosmic::widget::canvas;

/// Color bands per frame sample, sample-major. No colors draws a neutral track.
pub const COLOR_BANDS: usize = 24;

/// Height of the colored track.
pub const TRACK_HEIGHT: f32 = 34.0;
/// Total height, including the marker and time labels.
pub const HEIGHT: f32 = 62.0;
/// Width of either visible trim handle.
pub const HANDLE_WIDTH: f32 = 14.0;

const LABEL_BAND: f32 = HEIGHT - TRACK_HEIGHT;
const LABEL_SIZE: f32 = 11.0;
/// Monospace glyphs advance about 0.6 em; used to keep edge labels inside the widget.
const LABEL_GLYPH_WIDTH: f32 = LABEL_SIZE * 0.6;
const FRAME_BORDER: f32 = 3.0;
const OUTER_RADIUS: f32 = 6.0;
const INNER_RADIUS: f32 = 6.0;
const HANDLE_HIT_RADIUS: f32 = 24.0;
const DRAG_THRESHOLD: f32 = 3.0;
const ACTIVE_TINT: f32 = 0.14;
const PLAYHEAD_WIDTH: f32 = 2.0;
const MARKER_RADIUS: f32 = 4.0;
const CHEVRON_REACH: f32 = 4.0;
const CHEVRON_THICKNESS: f32 = 1.5;
const DEFAULT_MIN_SPAN: f64 = 1.0 / 60.0;

const _: () = {
    assert!(HANDLE_WIDTH < TRACK_HEIGHT);
    assert!(FRAME_BORDER * 2.0 < TRACK_HEIGHT);
    assert!(ACTIVE_TINT > 0.0 && ACTIVE_TINT < 1.0);
    assert!(CHEVRON_REACH * 2.0 + CHEVRON_THICKNESS < HANDLE_WIDTH);
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    Start,
    End,
}

#[derive(Debug, Clone, Copy)]
struct PendingDrag {
    handle: Handle,
    press_x: f32,
    started_on_handle: bool,
}

#[derive(Debug, Default)]
struct State {
    pending: Option<PendingDrag>,
    dragging: Option<Handle>,
    scrubbing: bool,
}

/// A media track with a trim-in handle, trim-out handle, and playhead.
pub struct TrackBar<'a, Message> {
    duration: f64,
    start: f64,
    end: f64,
    position: f64,
    min_span: f64,
    colors: &'a [[u8; 3]],
    on_seek: Option<Box<dyn Fn(f64) -> Message + 'a>>,
    on_seek_release: Option<Message>,
    on_trim: Option<Box<dyn Fn(f64, f64) -> Message + 'a>>,
    on_release: Option<Message>,
}

/// Creates a controlled media track. Values are clamped to `0..=duration`.
pub fn track_bar<Message>(
    duration: f64,
    start: f64,
    end: f64,
    position: f64,
) -> TrackBar<'static, Message> {
    TrackBar {
        duration: duration.max(f64::EPSILON),
        start,
        end,
        position,
        min_span: DEFAULT_MIN_SPAN,
        colors: &[],
        on_seek: None,
        on_seek_release: None,
        on_trim: None,
        on_release: None,
    }
}

impl<'a, Message> TrackBar<'a, Message> {
    /// Supplies sample-major frame colors for the track.
    #[must_use]
    pub const fn colors(mut self, colors: &'a [[u8; 3]]) -> Self {
        self.colors = colors;
        self
    }

    /// Shortest permitted span, in seconds. Normally one frame's duration.
    #[must_use]
    pub const fn minimum_span(mut self, seconds: f64) -> Self {
        self.min_span = seconds.clamp(f64::EPSILON, self.duration);
        self
    }

    /// Fires for clicks anywhere on the track and continuously while a handle moves.
    #[must_use]
    pub fn on_seek(mut self, f: impl Fn(f64) -> Message + 'a) -> Self {
        self.on_seek = Some(Box::new(f));
        self
    }

    /// Emits when a seek is released, for one accurate seek after fast previews.
    #[must_use]
    pub fn on_seek_release(mut self, message: Message) -> Self {
        self.on_seek_release = Some(message);
        self
    }

    /// Sends the new controlled trim range while a handle moves.
    #[must_use]
    pub fn on_trim(mut self, f: impl Fn(f64, f64) -> Message + 'a) -> Self {
        self.on_trim = Some(Box::new(f));
        self
    }

    /// Sends a message after a handle drag ends.
    #[must_use]
    pub fn on_release(mut self, message: Message) -> Self {
        self.on_release = Some(message);
        self
    }

    const fn range(&self) -> (f64, f64) {
        let start = self.start.clamp(0.0, self.duration);
        let end = self.end.clamp(start, self.duration);
        (start, end)
    }

    /// The time axis excludes the two outer handle gutters.
    fn axis(bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x + HANDLE_WIDTH,
            width: HANDLE_WIDTH.mul_add(-2.0, bounds.width).max(0.0),
            height: TRACK_HEIGHT,
            ..bounds
        }
    }

    fn x_of(&self, axis: Rectangle, time: f64) -> f32 {
        let ratio = (time / self.duration).clamp(0.0, 1.0) as f32;
        axis.x + ratio * axis.width
    }

    fn time_of(&self, axis: Rectangle, x: f32) -> f64 {
        if axis.width <= 0.0 {
            return 0.0;
        }
        let ratio = ((x - axis.x) / axis.width).clamp(0.0, 1.0);
        f64::from(ratio) * self.duration
    }

    fn handle_bounds(&self, axis: Rectangle, handle: Handle) -> Rectangle {
        let (start, end) = self.range();
        match handle {
            Handle::Start => {
                let edge = self.x_of(axis, start);
                Rectangle {
                    x: edge - HANDLE_WIDTH,
                    y: axis.y,
                    width: HANDLE_WIDTH,
                    height: axis.height,
                }
            }
            Handle::End => Rectangle {
                x: self.x_of(axis, end),
                y: axis.y,
                width: HANDLE_WIDTH,
                height: axis.height,
            },
        }
    }

    /// One continuous accent shape containing both handles and their bridge.
    fn frame_bounds(&self, axis: Rectangle) -> Rectangle {
        let (start, end) = self.range();
        let start_x = self.x_of(axis, start);
        let end_x = self.x_of(axis, end);
        Rectangle {
            x: start_x - HANDLE_WIDTH,
            y: axis.y,
            width: HANDLE_WIDTH.mul_add(2.0, end_x - start_x),
            height: axis.height,
        }
    }

    fn visible_handle_at(&self, axis: Rectangle, point: Point) -> Option<Handle> {
        [Handle::Start, Handle::End]
            .into_iter()
            .find(|handle| self.handle_bounds(axis, *handle).contains(point))
    }

    /// When the enlarged targets overlap, the nearer edge is chosen.
    fn handle_at(&self, axis: Rectangle, x: f32) -> Option<Handle> {
        let (start, end) = self.range();
        let start_x = self.x_of(axis, start);
        let end_x = self.x_of(axis, end);
        let start_distance = (x - start_x).abs();
        let end_distance = (x - end_x).abs();
        let can_start = start_distance <= HANDLE_HIT_RADIUS;
        let can_end = end_distance <= HANDLE_HIT_RADIUS;

        match (can_start, can_end) {
            (false, false) => None,
            (true, false) => Some(Handle::Start),
            (false, true) => Some(Handle::End),
            (true, true) if start_distance < end_distance => Some(Handle::Start),
            (true, true) if end_distance < start_distance => Some(Handle::End),
            (true, true) if x <= f32::midpoint(start_x, end_x) => Some(Handle::Start),
            (true, true) => Some(Handle::End),
        }
    }

    fn publish_seek(&self, shell: &mut Shell<'_, Message>, time: f64) {
        if let Some(ref f) = self.on_seek {
            shell.publish(f(time.clamp(0.0, self.duration)));
        }
    }

    fn publish_drag(&self, shell: &mut Shell<'_, Message>, handle: Handle, time: f64) {
        let (start, end) = self.range();
        let min_span = self.min_span.min(self.duration);
        let (new_start, new_end, preview) = match handle {
            Handle::Start => {
                let next = time.clamp(0.0, (end - min_span).max(0.0));
                (next, end, next)
            }
            Handle::End => {
                let next = time.clamp((start + min_span).min(self.duration), self.duration);
                (start, next, next)
            }
        };

        if let Some(ref f) = self.on_trim {
            shell.publish(f(new_start, new_end));
        }
        self.publish_seek(shell, preview);
    }

    fn draw_strip(&self, renderer: &mut cosmic::Renderer, axis: Rectangle, clip: Rectangle) {
        if axis.width <= 0.0 || clip.width <= 0.0 || clip.height <= 0.0 {
            return;
        }
        let samples = self.colors.len() / COLOR_BANDS;
        if samples == 0 {
            fill(renderer, clip, Color::from_rgb(0.24, 0.30, 0.32), 0.0);
            return;
        }

        let band_height = clip.height / COLOR_BANDS as f32;
        let clip_right = clip.x + clip.width;
        for sample in 0..samples {
            let sample_left = axis.x + axis.width * sample as f32 / samples as f32;
            let sample_right = axis.x + axis.width * (sample + 1) as f32 / samples as f32;
            let left = sample_left.max(clip.x);
            let right = sample_right.min(clip_right);
            if right <= left {
                continue;
            }
            for band in 0..COLOR_BANDS {
                let [r, g, b] = self.colors[sample * COLOR_BANDS + band];
                fill(
                    renderer,
                    Rectangle {
                        x: left,
                        y: (band as f32).mul_add(band_height, clip.y),
                        width: right - left + 0.5,
                        height: band_height + 0.5,
                    },
                    Color::from_rgb8(r, g, b),
                    0.0,
                );
            }
        }
    }

    fn draw_label(
        renderer: &mut cosmic::Renderer,
        full: Rectangle,
        x: f32,
        time: f64,
        color: Color,
    ) {
        let content = format_time(time);
        // Centered on the tick, but nudged inward at the ends so the clip
        // rectangle (`full`) never cuts the outer digits off.
        let half = LABEL_GLYPH_WIDTH * content.len() as f32 / 2.0 + 1.0;
        let x = if half * 2.0 >= full.width {
            full.center_x()
        } else {
            x.clamp(full.x + half, full.x + full.width - half)
        };
        renderer.fill_text(
            cosmic::iced::core::text::Text {
                content,
                bounds: Size::new(52.0, LABEL_BAND),
                size: cosmic::iced::Pixels(LABEL_SIZE),
                line_height: cosmic::iced::core::text::LineHeight::default(),
                font: cosmic::iced::Font::MONOSPACE,
                align_x: cosmic::iced::core::alignment::Horizontal::Center.into(),
                align_y: cosmic::iced::core::alignment::Vertical::Top,
                shaping: cosmic::iced::core::text::Shaping::Basic,
                wrapping: cosmic::iced::core::text::Wrapping::None,
                ellipsize: cosmic::iced::core::text::Ellipsize::default(),
            },
            Point::new(x, full.y + TRACK_HEIGHT + 10.0),
            color,
            full,
        );
    }
}

impl<Message: Clone> Widget<Message, cosmic::Theme, cosmic::Renderer> for TrackBar<'_, Message> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(HEIGHT))
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(limits.max().width, HEIGHT))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: Cursor,
        _viewport: &Rectangle,
    ) {
        let full = layout.bounds();
        let axis = Self::axis(full);
        if axis.width <= 0.0 {
            return;
        }

        let cosmic = theme.cosmic();
        let accent: Color = cosmic.accent_color().into();
        let on_accent: Color = cosmic.on_accent_color().into();
        let ink: Color = cosmic.background(false).on.into();
        let surrounding: Color = cosmic.background(false).base.into();
        let (start, end) = self.range();
        let start_x = self.x_of(axis, start);
        let end_x = self.x_of(axis, end);

        // The full media remains visible, even outside the exported range.
        self.draw_strip(renderer, axis, axis);
        draw_ring(renderer, axis, 0.0, axis, 4.0, surrounding);
        fill(
            renderer,
            Rectangle {
                width: (start_x - axis.x).max(0.0),
                ..axis
            },
            Color::from_rgba(0.0, 0.0, 0.0, 0.54),
            0.0,
        );
        fill(
            renderer,
            Rectangle {
                x: end_x,
                width: (axis.x + axis.width - end_x).max(0.0),
                ..axis
            },
            Color::from_rgba(0.0, 0.0, 0.0, 0.54),
            0.0,
        );

        // Selected frames first, then one even-odd path whose hole is the inner track.
        let kept = Rectangle {
            x: start_x,
            width: (end_x - start_x).max(0.0),
            ..axis
        };
        let inner = Rectangle {
            y: kept.y + FRAME_BORDER,
            height: FRAME_BORDER.mul_add(-2.0, kept.height),
            ..kept
        };
        self.draw_strip(renderer, axis, inner);
        fill(
            renderer,
            inner,
            Color {
                a: ACTIVE_TINT,
                ..accent
            },
            0.0,
        );
        draw_ring(
            renderer,
            self.frame_bounds(axis),
            OUTER_RADIUS,
            inner,
            INNER_RADIUS,
            accent,
        );

        // Start/end ticks mark the inner handle edges: these are the times.
        for (x, time) in [(start_x, start), (end_x, end)] {
            fill(
                renderer,
                Rectangle {
                    x: x - 0.5,
                    y: axis.y + axis.height,
                    width: 1.0,
                    height: 5.0,
                },
                Color { a: 0.5, ..ink },
                0.0,
            );
            Self::draw_label(renderer, full, x, time, Color { a: 0.72, ..ink });
        }

        // The playhead is independent of the trim range and may sit in a
        // discarded region while the user chooses a new cut point.
        let position = self.position.clamp(0.0, self.duration);
        let playhead_x = self.x_of(axis, position);
        fill(
            renderer,
            Rectangle {
                x: playhead_x - PLAYHEAD_WIDTH / 2.0,
                y: axis.y - 2.0,
                width: PLAYHEAD_WIDTH,
                height: axis.height + 6.0,
            },
            accent,
            0.0,
        );
        fill(
            renderer,
            Rectangle {
                x: playhead_x - MARKER_RADIUS,
                y: axis.y + axis.height + 1.0,
                width: MARKER_RADIUS * 2.0,
                height: MARKER_RADIUS * 2.0,
            },
            accent,
            MARKER_RADIUS,
        );
        Self::draw_label(renderer, full, playhead_x, position, accent);

        // A white hairline previews the timestamp under the pointer without
        // changing the controlled playhead until it is clicked.
        if let Some(point) = cursor.position_over(axis) {
            fill(
                renderer,
                Rectangle {
                    x: point.x - 0.5,
                    y: axis.y,
                    width: 1.0,
                    height: axis.height,
                },
                Color::from_rgba(1.0, 1.0, 1.0, 0.78),
                0.0,
            );
        }

        let state = tree.state.downcast_ref::<State>();
        let hovered = cursor
            .position_over(Rectangle {
                x: full.x,
                y: full.y,
                width: full.width,
                height: TRACK_HEIGHT,
            })
            .and_then(|point| self.handle_at(axis, point.x));
        renderer.with_layer(full, |renderer| {
            for handle in [Handle::Start, Handle::End] {
                let live = state.dragging == Some(handle) || hovered == Some(handle);
                let bounds = self.handle_bounds(axis, handle);
                chevron(
                    renderer,
                    bounds.center(),
                    handle == Handle::Start,
                    if live { Color::BLACK } else { on_accent },
                );
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &cosmic::Renderer,
        _clipboard: &mut dyn cosmic::iced::core::Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let full = layout.bounds();
        let axis = Self::axis(full);
        let state = tree.state.downcast_mut::<State>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                let Some(point) = event_point(event, cursor) else {
                    return;
                };
                let interaction_bounds = Rectangle {
                    height: TRACK_HEIGHT,
                    ..full
                };
                if !interaction_bounds.contains(point) {
                    return;
                }

                if let Some(handle) = self.handle_at(axis, point.x) {
                    state.pending = Some(PendingDrag {
                        handle,
                        press_x: point.x,
                        started_on_handle: self.visible_handle_at(axis, point).is_some(),
                    });
                } else if axis.contains(point) {
                    state.scrubbing = true;
                    self.publish_seek(shell, self.time_of(axis, point.x));
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if state.scrubbing {
                    if let Some(point) = event_point(event, cursor) {
                        self.publish_seek(shell, self.time_of(axis, point.x));
                        shell.capture_event();
                    }
                    return;
                }
                let Some(pending) = state.pending else {
                    return;
                };
                let Some(point) = event_point(event, cursor) else {
                    return;
                };
                if state.dragging.is_none() && (point.x - pending.press_x).abs() < DRAG_THRESHOLD {
                    return;
                }
                state.dragging = Some(pending.handle);
                self.publish_drag(shell, pending.handle, self.time_of(axis, point.x));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. }) => {
                if state.scrubbing {
                    state.scrubbing = false;
                    if let Some(message) = self.on_seek_release.clone() {
                        shell.publish(message);
                    }
                    shell.capture_event();
                    return;
                }
                let Some(pending) = state.pending.take() else {
                    return;
                };
                if state.dragging.take().is_some() {
                    if let Some(message) = self.on_release.clone() {
                        shell.publish(message);
                    }
                } else if pending.started_on_handle {
                    let (start, end) = self.range();
                    self.publish_seek(
                        shell,
                        match pending.handle {
                            Handle::Start => start,
                            Handle::End => end,
                        },
                    );
                    if let Some(message) = self.on_seek_release.clone() {
                        shell.publish(message);
                    }
                } else {
                    // An enlarged hit target must not steal a stationary click
                    // intended for a nearby discarded frame.
                    self.publish_seek(shell, self.time_of(axis, pending.press_x));
                    if let Some(message) = self.on_seek_release.clone() {
                        shell.publish(message);
                    }
                }
                shell.capture_event();
            }
            Event::Touch(touch::Event::FingerLost { .. }) => {
                state.pending = None;
                state.dragging = None;
                state.scrubbing = false;
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        _viewport: &Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.dragging.is_some() || state.pending.is_some() {
            return mouse::Interaction::ResizingHorizontally;
        }
        if state.scrubbing {
            return mouse::Interaction::Crosshair;
        }
        let axis = Self::axis(layout.bounds());
        cursor
            .position()
            .map_or_else(mouse::Interaction::default, |point| {
                if point.y >= axis.y
                    && point.y <= axis.y + axis.height
                    && self.handle_at(axis, point.x).is_some()
                {
                    mouse::Interaction::ResizingHorizontally
                } else if axis.contains(point) {
                    mouse::Interaction::Crosshair
                } else {
                    mouse::Interaction::default()
                }
            })
    }
}

impl<'a, Message: Clone + 'a> From<TrackBar<'a, Message>>
    for Element<'a, Message, cosmic::Theme, cosmic::Renderer>
{
    fn from(track: TrackBar<'a, Message>) -> Self {
        Element::new(track)
    }
}

fn event_point(event: &Event, cursor: Cursor) -> Option<Point> {
    match event {
        Event::Touch(
            touch::Event::FingerPressed { position, .. }
            | touch::Event::FingerMoved { position, .. }
            | touch::Event::FingerLifted { position, .. }
            | touch::Event::FingerLost { position, .. },
        ) => Some(*position),
        _ => cursor.position(),
    }
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let minutes = total / 60;
    let seconds = total % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn fill(renderer: &mut cosmic::Renderer, bounds: Rectangle, color: Color, radius: f32) {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
    }
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border {
                radius: radius.into(),
                ..Default::default()
            },
            shadow: cosmic::iced::core::Shadow::default(),
            snap: false,
        },
        Background::Color(color),
    );
}

/// Draws one anti-aliased shape between rounded outer and inner boundaries.
fn draw_ring(
    renderer: &mut cosmic::Renderer,
    outer: Rectangle,
    outer_radius: f32,
    inner: Rectangle,
    inner_radius: f32,
    color: Color,
) {
    if outer.width <= 0.0 || outer.height <= 0.0 || inner.width <= 0.0 || inner.height <= 0.0 {
        return;
    }

    let path = canvas::Path::new(|path| {
        path.rounded_rectangle(outer.position(), outer.size(), outer_radius.into());
        path.rounded_rectangle(inner.position(), inner.size(), inner_radius.into());
    });
    let mut frame = canvas::Frame::with_bounds(renderer, outer);
    frame.fill(
        &path,
        canvas::Fill {
            style: canvas::Style::Solid(color),
            rule: canvas::fill::Rule::EvenOdd,
        },
    );
    renderer.draw_geometry(frame.into_geometry());
}

fn chevron(renderer: &mut cosmic::Renderer, center: Point, pointing_right: bool, color: Color) {
    let direction = if pointing_right { 1.0 } else { -1.0 };
    let tip = Point::new(center.x + direction * CHEVRON_REACH / 2.0, center.y);
    for arm in [-1.0_f32, 1.0] {
        let from = Point::new(
            tip.x - direction * CHEVRON_REACH,
            arm.mul_add(CHEVRON_REACH, tip.y),
        );
        stroke(renderer, from, tip, CHEVRON_THICKNESS, color);
    }
}

fn stroke(renderer: &mut cosmic::Renderer, from: Point, to: Point, thickness: f32, color: Color) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = dx.hypot(dy);
    if length <= 0.0 {
        return;
    }
    let steps = (length / (thickness * 0.5)).ceil().max(1.0);
    for i in 0..=steps as u32 {
        let t = i as f32 / steps;
        fill(
            renderer,
            Rectangle {
                x: dx.mul_add(t, from.x) - thickness / 2.0,
                y: dy.mul_add(t, from.y) - thickness / 2.0,
                width: thickness,
                height: thickness,
            },
            color,
            thickness / 2.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: Rectangle = Rectangle {
        x: 10.0,
        y: 20.0,
        width: 128.0,
        height: HEIGHT,
    };

    fn bar(start: f64, end: f64) -> TrackBar<'static, ()> {
        track_bar(10.0, start, end, 5.0)
    }

    #[test]
    fn axis_excludes_handles() {
        let axis = TrackBar::<()>::axis(BOUNDS);
        assert_eq!(axis.x, BOUNDS.x + HANDLE_WIDTH);
        assert_eq!(axis.width, HANDLE_WIDTH.mul_add(-2.0, BOUNDS.width));
    }

    #[test]
    fn zero_after_left_handle() {
        let bar = bar(0.0, 10.0);
        let axis = TrackBar::<()>::axis(BOUNDS);
        let left = bar.handle_bounds(axis, Handle::Start);
        assert_eq!(left.x + left.width, bar.x_of(axis, 0.0));
        assert_eq!(bar.x_of(axis, 0.0), axis.x);
    }

    #[test]
    fn end_before_right_handle() {
        let bar = bar(0.0, 10.0);
        let axis = TrackBar::<()>::axis(BOUNDS);
        let right = bar.handle_bounds(axis, Handle::End);
        assert_eq!(right.x, bar.x_of(axis, 10.0));
        assert_eq!(right.x + right.width, BOUNDS.x + BOUNDS.width);
    }

    #[test]
    fn frame_spans_handles() {
        let bar = bar(2.0, 8.0);
        let axis = TrackBar::<()>::axis(BOUNDS);
        let frame = bar.frame_bounds(axis);
        let left = bar.handle_bounds(axis, Handle::Start);
        let right = bar.handle_bounds(axis, Handle::End);
        assert_eq!(frame.x, left.x);
        assert_eq!(frame.x + frame.width, right.x + right.width);
        assert_eq!(left.x + left.width, bar.x_of(axis, 2.0));
        assert_eq!(right.x, bar.x_of(axis, 8.0));
    }

    #[test]
    fn x_of_time_of_roundtrip() {
        let bar = bar(0.0, 10.0);
        let axis = TrackBar::<()>::axis(BOUNDS);
        for time in [0.0, 2.5, 5.0, 9.9, 10.0] {
            assert!((bar.time_of(axis, bar.x_of(axis, time)) - time).abs() < 1e-6);
        }
    }

    #[test]
    fn close_handles_stay_grabbable() {
        let bar = bar(4.999, 5.001);
        let axis = TrackBar::<()>::axis(BOUNDS);
        let start_x = bar.x_of(axis, 4.999);
        let end_x = bar.x_of(axis, 5.001);
        assert_eq!(bar.handle_at(axis, start_x - 4.0), Some(Handle::Start));
        assert_eq!(bar.handle_at(axis, end_x + 4.0), Some(Handle::End));
    }

    #[test]
    fn overlap_split_by_side() {
        let bar = bar(5.0, 5.0);
        let axis = TrackBar::<()>::axis(BOUNDS);
        let edge = bar.x_of(axis, 5.0);
        assert_eq!(bar.handle_at(axis, edge - 0.1), Some(Handle::Start));
        assert_eq!(bar.handle_at(axis, edge + 0.1), Some(Handle::End));
    }

    #[test]
    fn min_span_clamped_to_duration() {
        let bar = bar(0.0, 10.0).minimum_span(50.0);
        assert_eq!(bar.min_span, bar.duration);
    }

    #[test]
    fn labels_cross_the_minute_boundary() {
        assert_eq!(format_time(0.0), "00:00");
        assert_eq!(format_time(65.2), "01:05");
    }
}

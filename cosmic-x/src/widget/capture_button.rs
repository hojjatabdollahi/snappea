// SPDX-License-Identifier: GPL-3.0-only

//! The shutter button: a ring around a disc, animated in place.

use cosmic::iced::advanced::graphics::geometry::Renderer as GeometryRenderer;
use cosmic::iced::animation::Easing;
use cosmic::iced::core::{
    Background, Border, Element, Layout, Length, Rectangle, Shell, Size, Widget,
    event::Event,
    layout, mouse,
    renderer::{self, Quad, Renderer as RendererTrait},
    touch,
    widget::tree::{self, Tree},
    window,
};
use cosmic::iced::widget::canvas;
use cosmic::iced::{Animation, Color, Point};
use std::time::{Duration, Instant};

/// Diameter of the ring, and so of the whole button. Public for the toolbar
/// and the recording chrome, which size around it.
pub const SIZE: f32 = 36.0;
const RING: f32 = SIZE;
/// Ring thickness.
const RING_WIDTH: f32 = 1.0;
/// Diameter of the disc inside it when at rest.
const DISC: f32 = RING - 10.0;
/// How much bigger the disc gets while held.
const PRESS_GROWTH: f32 = 0.18;
/// The recording red, shared with the stop button. Not themed.
const RECORD_RED: Color = Color::from_rgb(0.85, 0.2, 0.2);

// The pressed disc, and the pressed square's diagonal, stay inside the ring.
const _: () = {
    assert!(DISC * (1.0 + PRESS_GROWTH) < RING);
    assert!(DISC * STOP_SCALE * (1.0 + PRESS_GROWTH) * 1.4143 < RING - RING_WIDTH * 2.0);
    // A shutter should answer at once and relax afterwards.
    assert!(PRESS_OUT.as_millis() > PRESS_IN.as_millis());
    // The countdown ring unwinds inside the button's own ring, never across it.
    assert!(COUNT_R0 + COUNT_WIDTH / 2.0 < RING / 2.0 - RING_WIDTH);
    // It starts outside the disc it replaces, so they read as separate marks.
    assert!(COUNT_R0 > DISC / 2.0);
};

/// How quickly the disc swells under a press, and settles back after one.
const PRESS_IN: Duration = Duration::from_millis(90);
const PRESS_OUT: Duration = Duration::from_millis(220);
/// How quickly the disc rounds off into a stop square, and reddens.
const MORPH: Duration = Duration::from_millis(240);

/// Corner radius of the stop square, as a fraction of its side.
const STOP_RADIUS: f32 = 1.0 / 6.0;

/// Radius of the countdown ring, inside the button.
const COUNT_R0: f32 = 15.0;
/// Thickness of the countdown ring.
const COUNT_WIDTH: f32 = 2.0;

/// How much the disc shrinks as it squares off, so the square fits inside the circle.
const STOP_SCALE: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// What the button is currently for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Takes a screenshot. Pale disc.
    Photo,
    /// Will start a recording. Red disc.
    Ready,
    /// Recording now. Red square, accent ring.
    Recording,
    /// Waiting out a delayed capture. Carries the wait itself so the ring reads the
    /// same deadline the shutter fires on.
    Counting {
        started: Instant,
        duration: Duration,
    },
}

impl Mode {
    #[must_use]
    pub const fn of(video: bool, recording: bool) -> Self {
        match (video, recording) {
            (_, true) => Self::Recording,
            (true, false) => Self::Ready,
            (false, false) => Self::Photo,
        }
    }

    /// How red the disc is, how square, and how far given over to the
    /// countdown.
    const fn targets(self) -> (f32, f32, f32) {
        match self {
            Self::Photo => (0.0, 0.0, 0.0),
            Self::Ready => (1.0, 0.0, 0.0),
            Self::Recording => (1.0, 1.0, 0.0),
            Self::Counting { .. } => (0.0, 0.0, 1.0),
        }
    }

    /// How much of the wait has gone, 0 to 1, or `None` if none is running. Saturates at 1.
    #[must_use]
    fn elapsed(self, now: Instant) -> Option<f32> {
        let Self::Counting { started, duration } = self else {
            return None;
        };
        if duration.is_zero() {
            return Some(1.0);
        }
        let waited = now.saturating_duration_since(started);
        Some((waited.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0))
    }

    /// Whether this is a wait, and it is not over yet.
    #[must_use]
    fn is_counting(self, now: Instant) -> bool {
        self.elapsed(now).is_some_and(|t| t < 1.0)
    }
}

/// The widget's own clock and animations, kept in the tree across rebuilds.
struct State {
    /// 0 pale, 1 red.
    red: Animation<f32>,
    /// 0 disc, 1 stop square with an accent ring.
    stop: Animation<f32>,
    /// 0 at rest, 1 fully swelled.
    press: Animation<f32>,
    /// 0 disc, 1 countdown ring.
    count: Animation<f32>,
    held: bool,
    now: Instant,
    /// What the last frame was told the mode was, so a change can be noticed
    /// without the app having to send one.
    mode: Mode,
}

impl State {
    fn new(mode: Mode) -> Self {
        let (red, stop, count) = mode.targets();
        Self {
            red: Animation::new(red).easing(Easing::EaseOut).duration(MORPH),
            stop: Animation::new(stop).easing(Easing::EaseOut).duration(MORPH),
            press: Animation::new(0.0)
                .easing(Easing::EaseOut)
                .duration(PRESS_IN),
            count: Animation::new(count)
                .easing(Easing::EaseOut)
                .duration(MORPH),
            held: false,
            now: Instant::now(),
            mode,
        }
    }

    fn is_animating(&self) -> bool {
        self.red.is_animating(self.now)
            || self.stop.is_animating(self.now)
            || self.press.is_animating(self.now)
            || self.count.is_animating(self.now)
            // A wait keeps asking for frames until the ring has closed.
            || self.mode.is_counting(self.now)
    }

    /// Point the animations at a mode, if it is not where they are already going.
    fn retarget(&mut self, mode: Mode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        let (red, stop, count) = mode.targets();
        self.red.go_mut(red, self.now);
        self.stop.go_mut(stop, self.now);
        self.count.go_mut(count, self.now);
    }

    /// Start the press or release animation. The release is slower than the
    /// press, so the button responds at once and eases back.
    fn set_held(&mut self, held: bool) {
        if self.held == held {
            return;
        }
        self.held = held;
        self.press = Animation::new(self.press.interpolate_with(|v| v, self.now))
            .easing(Easing::EaseOut)
            .duration(if held { PRESS_IN } else { PRESS_OUT });
        self.press.go_mut(if held { 1.0 } else { 0.0 }, self.now);
    }
}

/// The shutter button.
pub struct CaptureButton<Msg> {
    mode: Mode,
    on_press: Option<Msg>,
    transition: Option<(Mode, f32)>,
}

/// The shutter button.
pub const fn capture_button<Msg>(mode: Mode) -> CaptureButton<Msg> {
    CaptureButton {
        mode,
        on_press: None,
        transition: None,
    }
}

impl<Msg> CaptureButton<Msg> {
    /// What to send when it is pressed. Without one it is inert and dimmed.
    #[must_use]
    pub fn on_press_maybe(mut self, message: Option<Msg>) -> Self {
        self.on_press = message;
        self
    }

    /// Draw an externally driven morph from `mode` into this button's mode, for
    /// the tree change at recording start.
    #[must_use]
    pub const fn transition_from(mut self, mode: Mode, progress: f32) -> Self {
        self.transition = Some((mode, progress.clamp(0.0, 1.0)));
        self
    }
}

impl<Msg: Clone> Widget<Msg, cosmic::Theme, cosmic::Renderer> for CaptureButton<Msg> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(RING), Length::Fixed(RING))
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new(self.mode))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(RING, RING))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let cosmic = theme.cosmic();

        let (red, stop, forced_count) = self.transition.map_or_else(
            || {
                (
                    state.red.interpolate_with(|v| v, state.now),
                    state.stop.interpolate_with(|v| v, state.now),
                    state.count.interpolate_with(|v| v, state.now),
                )
            },
            |(from, progress)| {
                let from = from.targets();
                let to = self.mode.targets();
                let at = |a: f32, b: f32| (b - a).mul_add(progress, a);
                (at(from.0, to.0), at(from.1, to.1), at(from.2, to.2))
            },
        );
        let press = state.press.interpolate_with(|v| v, state.now);
        let count = forced_count;

        let symbolic: Color = cosmic.background(false).component.on.into();
        let accent: Color = cosmic.accent_color().into();
        // Dimmed when there is nothing to press, but never while counting.
        let alpha = if self.on_press.is_some() || count > 0.0 {
            1.0
        } else {
            0.4
        };

        // The ring uses the symbolic icon color and shifts to the accent color
        // as the recording starts.
        let ring = fade(mix(symbolic, accent, stop), alpha);
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.center_x() - RING / 2.0,
                    y: bounds.center_y() - RING / 2.0,
                    width: RING,
                    height: RING,
                },
                border: Border {
                    radius: (RING / 2.0).into(),
                    width: RING_WIDTH,
                    color: ring,
                },
                shadow: cosmic::iced::core::Shadow::default(),
                snap: false,
            },
            Background::Color(Color::TRANSPARENT),
        );

        // The disc is pale for a screenshot and red for a recording, and becomes a square once recording.
        let size = DISC * PRESS_GROWTH.mul_add(press, 1.0) * (1.0 - STOP_SCALE).mul_add(-stop, 1.0);
        let radius = size * (STOP_RADIUS - 0.5).mul_add(stop, 0.5);
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.center_x() - size / 2.0,
                    y: bounds.center_y() - size / 2.0,
                    width: size,
                    height: size,
                },
                border: Border {
                    radius: radius.into(),
                    ..Default::default()
                },
                shadow: cosmic::iced::core::Shadow::default(),
                snap: false,
            },
            // Gives way to the countdown ring, which occupies the same space.
            Background::Color(fade(mix(symbolic, RECORD_RED, red), alpha * (1.0 - count))),
        );

        // The countdown ring shrinks to nothing as time runs out.
        if let Some(elapsed) = self.mode.elapsed(state.now) {
            let sweep = std::f32::consts::TAU * (1.0 - elapsed);
            if sweep > 0.0 {
                // From twelve o'clock, clockwise, like anything else that
                // counts down on a face.
                let start = -std::f32::consts::FRAC_PI_2;
                let path = canvas::Path::new(|path| {
                    path.arc(canvas::path::Arc {
                        center: Point::new(bounds.center_x(), bounds.center_y()),
                        radius: COUNT_R0,
                        start_angle: start.into(),
                        end_angle: (start + sweep).into(),
                    });
                });
                let mut frame = canvas::Frame::with_bounds(renderer, bounds);
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(fade(accent, alpha * count))
                        .with_width(COUNT_WIDTH),
                );
                renderer.draw_geometry(frame.into_geometry());
            }
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &cosmic::Renderer,
        _clipboard: &mut dyn cosmic::iced::core::Clipboard,
        shell: &mut Shell<'_, Msg>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();

        match event {
            // Ask for frames only while something is moving.
            Event::Window(window::Event::RedrawRequested(now)) => {
                state.now = *now;
                state.retarget(self.mode);
                if state.is_animating() {
                    shell.request_redraw();
                }
                return;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if self.on_press.is_some() && cursor.is_over(bounds) {
                    state.set_held(true);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. })
                if state.held =>
            {
                {
                    state.set_held(false);
                    shell.request_redraw();
                    // Only a release over the button is a press. Dragging off
                    // it is how a press is taken back.
                    if cursor.is_over(bounds)
                        && let Some(message) = self.on_press.clone()
                    {
                        shell.publish(message);
                        shell.capture_event();
                    }
                }
            }
            _ => {}
        }

        // A mode set between frames starts animating now, not on the next redraw.
        if state.mode != self.mode {
            state.retarget(self.mode);
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        if self.on_press.is_some() && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Msg: Clone + 'a> From<CaptureButton<Msg>>
    for Element<'a, Msg, cosmic::Theme, cosmic::Renderer>
{
    fn from(button: CaptureButton<Msg>) -> Self {
        Element::new(button)
    }
}

/// Blend two colors, `t` of the way from `a` to `b`.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: (b.r - a.r).mul_add(t, a.r),
        g: (b.g - a.g).mul_add(t, a.g),
        b: (b.b - a.b).mul_add(t, a.b),
        a: (b.a - a.a).mul_add(t, a.a),
    }
}

fn fade(color: Color, alpha: f32) -> Color {
    Color {
        a: color.a * alpha,
        ..color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_sets_disc_target() {
        assert_eq!(Mode::of(false, false), Mode::Photo);
        assert_eq!(Mode::of(true, false), Mode::Ready);
        assert_eq!(Mode::of(true, true), Mode::Recording);
        // A recording is a recording however it was started.
        assert_eq!(Mode::of(false, true), Mode::Recording);
    }

    #[test]
    fn screenshot_disc_recording_square() {
        assert_eq!(Mode::Photo.targets(), (0.0, 0.0, 0.0));
        assert_eq!(
            Mode::Ready.targets(),
            (1.0, 0.0, 0.0),
            "red, but still a disc"
        );
        assert_eq!(Mode::Recording.targets(), (1.0, 1.0, 0.0));
    }

    fn counting(secs: u64) -> (Mode, Instant) {
        let started = Instant::now();
        (
            Mode::Counting {
                started,
                duration: Duration::from_secs(secs),
            },
            started,
        )
    }

    #[test]
    fn wait_shows_countdown() {
        let (mode, _) = counting(3);
        assert_eq!(mode.targets(), (0.0, 0.0, 1.0), "no disc, all ring");
    }

    #[test]
    fn ring_closes_linearly() {
        // Linear over the whole wait.
        let (mode, start) = counting(4);
        assert_eq!(mode.elapsed(start), Some(0.0));
        assert_eq!(mode.elapsed(start + Duration::from_secs(1)), Some(0.25));
        assert_eq!(mode.elapsed(start + Duration::from_secs(2)), Some(0.5));
        assert_eq!(mode.elapsed(start + Duration::from_secs(4)), Some(1.0));
    }

    #[test]
    fn finished_wait_stays_finished() {
        // A frame after the countdown ended must still read as finished. It must
        // not wrap around into a new countdown.
        let (mode, start) = counting(2);
        assert_eq!(mode.elapsed(start + Duration::from_secs(9)), Some(1.0));
        assert!(!mode.is_counting(start + Duration::from_secs(9)));
        assert!(mode.is_counting(start + Duration::from_millis(500)));
    }

    #[test]
    fn ring_only_during_wait() {
        assert_eq!(Mode::Photo.elapsed(Instant::now()), None);
        assert!(!Mode::Recording.is_counting(Instant::now()));
    }

    #[test]
    fn counting_requests_redraws() {
        // A countdown is not an animation between two states, so `is_animating`
        // has to check the mode to know one is running.
        let (mode, start) = counting(5);
        let mut state = State::new(mode);
        state.now = start + Duration::from_secs(1);
        assert!(state.is_animating(), "still closing");
        state.now = start + Duration::from_secs(6);
        assert!(!state.is_animating(), "closed");
    }

    #[test]
    fn settled_requests_no_redraws() {
        let state = State::new(Mode::Photo);
        assert!(!state.is_animating(), "nothing to draw, nothing to request");
    }

    #[test]
    fn mode_change_animates_to_end() {
        let mut state = State::new(Mode::Photo);
        let start = state.now;
        state.retarget(Mode::Recording);
        assert!(state.is_animating());

        state.now = start + MORPH + Duration::from_millis(1);
        assert!(!state.is_animating());
        assert_eq!(state.stop.interpolate_with(|v| v, state.now), 1.0);
        assert_eq!(state.red.interpolate_with(|v| v, state.now), 1.0);
    }

    #[test]
    fn same_mode_is_noop() {
        let mut state = State::new(Mode::Photo);
        state.retarget(Mode::Photo);
        assert!(!state.is_animating());
    }

    #[test]
    fn press_swells_release_settles() {
        let mut state = State::new(Mode::Photo);
        let start = state.now;

        state.set_held(true);
        state.now = start + PRESS_IN + Duration::from_millis(1);
        assert_eq!(state.press.interpolate_with(|v| v, state.now), 1.0);

        state.set_held(false);
        let released = state.now;
        state.now = released + PRESS_OUT + Duration::from_millis(1);
        assert_eq!(state.press.interpolate_with(|v| v, state.now), 0.0);
    }

    #[test]
    fn release_settles_from_midway() {
        // A press released before it finished should relax from where it is,
        // not jump to fully swelled and fall from there.
        let mut state = State::new(Mode::Photo);
        let start = state.now;
        state.set_held(true);
        state.now = start + PRESS_IN / 3;
        let partway = state.press.interpolate_with(|v| v, state.now);
        assert!(partway > 0.0 && partway < 1.0, "caught mid-swell");

        state.set_held(false);
        let at_release = state.press.interpolate_with(|v| v, state.now);
        assert!(
            (at_release - partway).abs() < 0.01,
            "jumped from {partway} to {at_release}"
        );
    }

    #[test]
    fn colors_interpolate() {
        let a = Color::from_rgb(0.0, 0.0, 0.0);
        let b = Color::from_rgb(1.0, 0.5, 0.25);
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 2.0), b, "clamped, not extrapolated");
        let half = mix(a, b, 0.5);
        assert!((half.r - 0.5).abs() < 1e-6);
    }
}

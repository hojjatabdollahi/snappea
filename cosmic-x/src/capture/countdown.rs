// SPDX-License-Identifier: GPL-3.0-only

//! Countdown overlay for a delayed capture. Drawn on the capture-excluded
//! layer so it never appears in the screenshot.

use cosmic::iced::core::Rectangle;
use cosmic::iced::window;
use std::time::{Duration, Instant};

/// Clearance around the pill for its input zone, so edge buttons stay clickable.
const MARGIN: f32 = 8.0;

/// Match the capture toolbar's section animation.
pub const TOOLBAR_TRANSITION_DURATION: Duration = Duration::from_millis(180);

/// One output's countdown surface.
#[derive(Clone, Debug)]
pub struct CountdownWindow {
    /// The output's own overlay surface, borrowed for the wait.
    pub id: window::Id,
    pub output_name: String,
    pub output_size: (f32, f32),
    /// Top-left of the pill, in this output's local logical coordinates.
    pub pos: (f32, f32),
    /// Where the armed-delay toolbar was before it began paring itself down.
    pub transition_from_pos: (f32, f32),
    /// Input rectangle occupied by that outgoing toolbar.
    pub transition_bounds: Rectangle,
}

/// How far a delayed capture has left to wait.
#[derive(Clone, Debug)]
pub struct Countdown {
    /// When the wait began, and how long it runs. The capture fires on this,
    /// and so does everything that draws it.
    pub started: Instant,
    pub duration: Duration,
    /// How much of the annotation entry point was visible when capture began.
    pub outgoing_tools: f32,
    /// Becomes true after the outgoing toolbar controls have fully collapsed.
    pub toolbar_transition_finished: bool,
    /// One surface per output, all counting the same wait.
    pub windows: Vec<CountdownWindow>,
    /// Which output's pill is being dragged, if any.
    pub dragging: Option<window::Id>,
    /// Cursor offset within the pill when the drag began, resolved on the first motion.
    pub drag_offset: Option<(f32, f32)>,
    /// Whether the capture has already fired. The countdown stays up through the
    /// capture, since its surface is capture-excluded.
    pub fired: bool,
}

impl Countdown {
    #[must_use]
    pub fn new(seconds: u32, windows: Vec<CountdownWindow>, outgoing_tools: f32) -> Self {
        Self {
            started: Instant::now(),
            // A zero-length countdown would fire as it appeared.
            duration: Duration::from_secs(u64::from(seconds.max(1))),
            outgoing_tools: outgoing_tools.clamp(0.0, 1.0),
            toolbar_transition_finished: false,
            windows,
            dragging: None,
            drag_offset: None,
            fired: false,
        }
    }

    /// Whole seconds still to wait, rounded up and never zero.
    #[must_use]
    pub fn remaining(&self, now: Instant) -> u32 {
        let left = self
            .duration
            .saturating_sub(now.saturating_duration_since(self.started));
        (left.as_secs_f32().ceil() as u32).max(1)
    }

    /// Whether the wait is over and the shutter is due.
    #[must_use]
    pub fn is_over(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= self.duration
    }

    #[must_use]
    pub fn owns(&self, id: window::Id) -> bool {
        self.windows.iter().any(|w| w.id == id)
    }

    #[must_use]
    pub fn window(&self, id: window::Id) -> Option<&CountdownWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// Ease the toolbar handoff with the same cubic curve as its sections.
    #[must_use]
    pub fn toolbar_transition_progress(&self, now: Instant) -> f32 {
        if self.toolbar_transition_finished {
            return 1.0;
        }
        let linear = now.saturating_duration_since(self.started).as_secs_f32()
            / TOOLBAR_TRANSITION_DURATION.as_secs_f32();
        let linear = linear.clamp(0.0, 1.0);
        1.0 - (1.0 - linear).powi(3)
    }

    /// Where a pill sits by default: centered along the bottom edge, as the
    /// toolbar it replaces does.
    #[must_use]
    pub fn default_pos(output_size: (f32, f32)) -> (f32, f32) {
        const BOTTOM_MARGIN: f32 = 32.0;
        let (w, h) = crate::widget::toolbar::countdown_toolbar_size();
        (
            ((output_size.0 - w) / 2.0).max(0.0),
            (output_size.1 - h - BOTTOM_MARGIN).max(0.0),
        )
    }
}

/// The area of the surface that accepts input: the pill plus a small margin.
#[must_use]
pub fn input_zone(pos: (f32, f32)) -> Rectangle {
    let (w, h) = crate::widget::toolbar::countdown_toolbar_size();
    input_zone_for_size(pos, (w, h))
}

/// Input rectangle for a toolbar whose dimensions are known before layout.
#[must_use]
pub fn input_zone_for_size(pos: (f32, f32), size: (f32, f32)) -> Rectangle {
    Rectangle {
        x: pos.0 - MARGIN,
        y: pos.1 - MARGIN,
        width: MARGIN.mul_add(2.0, size.0),
        height: MARGIN.mul_add(2.0, size.1),
    }
}

/// Clamp the pill position to its output.
#[must_use]
pub fn clamp_pos(pos: (f32, f32), output_size: (f32, f32)) -> (f32, f32) {
    let (w, h) = crate::widget::toolbar::countdown_toolbar_size();
    (
        pos.0.clamp(0.0, (output_size.0 - w).max(0.0)),
        pos.1.clamp(0.0, (output_size.1 - h).max(0.0)),
    )
}

impl CountdownWindow {
    #[must_use]
    pub fn input_zone(&self, transition_finished: bool) -> Rectangle {
        let final_zone = input_zone(self.pos);
        if transition_finished {
            return final_zone;
        }

        let from = self.transition_bounds;
        let left = from.x.min(final_zone.x);
        let top = from.y.min(final_zone.y);
        let right = (from.x + from.width).max(final_zone.x + final_zone.width);
        let bottom = (from.y + from.height).max(final_zone.y + final_zone.height);
        Rectangle {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }

    #[must_use]
    pub fn displayed_pos(&self, transition: f32) -> (f32, f32) {
        let transition = transition.clamp(0.0, 1.0);
        (
            (self.pos.0 - self.transition_from_pos.0)
                .mul_add(transition, self.transition_from_pos.0),
            (self.pos.1 - self.transition_from_pos.1)
                .mul_add(transition, self.transition_from_pos.1),
        )
    }

    /// Move it, and keep it on screen.
    pub fn move_to(&mut self, pos: (f32, f32)) {
        self.pos = clamp_pos(pos, self.output_size);
    }
}

/// What the pill sends back.
pub struct Handlers<Msg> {
    pub on_drag_start: Msg,
    pub on_drag_end: Msg,
    pub on_drag_move: Box<dyn Fn(f32, f32) -> Msg>,
    pub on_transition_noop: Msg,
    pub on_cancel: Msg,
}

/// The countdown pill, on the output that owns `id`.
#[must_use]
pub fn view<'a, Msg: Clone + 'static>(
    countdown: &Countdown,
    id: window::Id,
    now: Instant,
    handlers: Handlers<Msg>,
) -> cosmic::Element<'a, Msg> {
    use cosmic::iced::Length;
    use cosmic::iced::widget::{column, row, stack};

    let Some(window) = countdown.window(id) else {
        return cosmic::iced::widget::space()
            .width(Length::Fixed(1.0))
            .into();
    };
    let transition = countdown.toolbar_transition_progress(now);

    let pill =
        crate::widget::toolbar::build_countdown_toolbar(crate::widget::toolbar::CountdownToolbar {
            transition,
            outgoing_tools: countdown.outgoing_tools,
            started: countdown.started,
            duration: countdown.duration,
            remaining: countdown.remaining(now),
            on_drag_start: handlers.on_drag_start,
            on_drag_end: handlers.on_drag_end.clone(),
            on_transition_noop: handlers.on_transition_noop,
            on_cancel: handlers.on_cancel,
        });
    let pos = window.displayed_pos(transition);

    // Positioned with spacers, like the recording toolbar.
    let placed = column![
        cosmic::iced::widget::space().height(Length::Fixed(pos.1.max(0.0))),
        row![
            cosmic::iced::widget::space().width(Length::Fixed(pos.0.max(0.0))),
            pill,
            cosmic::iced::widget::space().width(Length::Fill),
        ]
        .width(Length::Fill),
        cosmic::iced::widget::space().height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    // Under the pill, so a drag continues past the input zone.
    let catcher = cosmic::iced::widget::canvas::Canvas::new(DragCatcher {
        dragging: countdown.dragging == Some(id),
        on_move: handlers.on_drag_move,
        on_release: handlers.on_drag_end,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    stack![catcher, placed]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Turns pointer motion into pill motion, and nothing else into anything.
struct DragCatcher<Msg> {
    dragging: bool,
    on_move: Box<dyn Fn(f32, f32) -> Msg>,
    on_release: Msg,
}

impl<Msg: Clone> cosmic::iced::widget::canvas::Program<Msg, cosmic::Theme> for DragCatcher<Msg> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &cosmic::iced::widget::canvas::Event,
        _bounds: Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Option<cosmic::iced::widget::canvas::Action<Msg>> {
        use cosmic::iced::core::mouse::{Button, Event as MouseEvent};
        use cosmic::iced::widget::canvas;

        if !self.dragging {
            return None;
        }

        match event {
            canvas::Event::Mouse(MouseEvent::CursorMoved { position }) => {
                Some(canvas::Action::publish((self.on_move)(position.x, position.y)).and_capture())
            }
            canvas::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                Some(canvas::Action::publish(self.on_release.clone()).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        _renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        _bounds: Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Vec<cosmic::iced::widget::canvas::Geometry> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn countdown(secs: u32) -> Countdown {
        Countdown::new(secs, Vec::new(), 0.0)
    }

    #[test]
    fn button_counts_down_seconds() {
        let c = countdown(3);
        let start = c.started;
        assert_eq!(c.remaining(start), 3);
        // Round up so a partial second still shows as one second.
        assert_eq!(c.remaining(start + SECOND / 2), 3);
        assert_eq!(c.remaining(start + SECOND), 2);
        assert_eq!(c.remaining(start + SECOND * 2), 1);
    }

    #[test]
    fn last_second_is_shown() {
        let c = countdown(3);
        assert_eq!(c.remaining(c.started + SECOND * 3), 1);
        assert_eq!(c.remaining(c.started + SECOND * 30), 1);
    }

    #[test]
    fn wait_ends_at_deadline() {
        let c = countdown(3);
        let start = c.started;
        assert!(!c.is_over(start));
        assert!(!c.is_over(start + SECOND * 2));
        assert!(c.is_over(start + SECOND * 3));
        assert!(c.is_over(start + SECOND * 4));
    }

    #[test]
    fn zero_delay_waits_one_second() {
        assert_eq!(countdown(0).duration, SECOND);
    }

    #[test]
    fn handoff_eases_open_to_closed() {
        let c = countdown(3);
        assert_eq!(c.toolbar_transition_progress(c.started), 0.0);
        let halfway = c.toolbar_transition_progress(c.started + TOOLBAR_TRANSITION_DURATION / 2);
        assert!(halfway > 0.5 && halfway < 1.0, "ease-out: {halfway}");
        assert_eq!(
            c.toolbar_transition_progress(c.started + TOOLBAR_TRANSITION_DURATION),
            1.0
        );
    }

    #[test]
    fn seconds_are_uniform() {
        let c = countdown(5);
        let start = c.started;
        let mut boundaries = Vec::new();
        let mut last = c.remaining(start);
        for ms in 0..5_000 {
            let at = start + Duration::from_millis(ms);
            let now = c.remaining(at);
            if now != last {
                boundaries.push(ms);
                last = now;
            }
        }

        assert_eq!(boundaries.len(), 4, "5, 4, 3, 2, 1: four changes");
        for pair in boundaries.windows(2) {
            assert_eq!(pair[1] - pair[0], 1_000, "{boundaries:?}");
        }
    }

    #[test]
    fn pill_stays_on_screen() {
        let (w, _) = crate::widget::toolbar::countdown_toolbar_size();
        let screen = (1920.0, 1080.0);
        let (x, y) = clamp_pos((5_000.0, -40.0), screen);
        assert!((x - (1920.0 - w)).abs() < 0.5, "dragged off the right");
        assert!(y.abs() < f32::EPSILON, "dragged off the top");
    }

    #[test]
    fn oversized_pill_pins_left() {
        // `clamp(0.0, negative)` panics on a screen narrower than the pill.
        let (x, y) = clamp_pos((40.0, 40.0), (100.0, 100.0));
        assert!(x.abs() < f32::EPSILON, "no room to move sideways");
        // Room vertically, so it is held inside.
        let (_, h) = crate::widget::toolbar::countdown_toolbar_size();
        assert!((y - (100.0 - h)).abs() < 0.5, "y: {y}");
    }

    #[test]
    fn input_zone_exceeds_pill() {
        // A zone cut exactly to the pill leaves the buttons at its edge dead.
        let (w, h) = crate::widget::toolbar::countdown_toolbar_size();
        let zone = input_zone((100.0, 200.0));
        assert!(zone.x < 100.0 && zone.y < 200.0);
        assert!(zone.width > w && zone.height > h);
        assert!(zone.contains(cosmic::iced::core::Point::new(100.0, 200.0)));
    }
}

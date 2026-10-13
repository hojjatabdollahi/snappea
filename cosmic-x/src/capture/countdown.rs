// SPDX-License-Identifier: GPL-3.0-only

//! Countdown overlay for a delayed capture. Drawn on the capture-excluded
//! layer so it never appears in the screenshot.

use crate::capture::ToolbarProgress;
use crate::geometry::Choice;
use cosmic::iced::core::Rectangle;
use cosmic::iced::window;
use std::time::{Duration, Instant};

/// Clearance around the toolbar for its input zone, so edge buttons stay clickable.
const MARGIN: f32 = 8.0;

pub const TOOLBAR_TRANSITION_DURATION: Duration = Duration::from_millis(180);

/// What the countdown reacts to. Wrapped in `app::Msg::Countdown`.
#[derive(Debug, Clone, Copy)]
pub enum Msg {
    /// A delayed capture's countdown finished. Carries the countdown's start time,
    /// so a timer left over from a cancelled countdown cannot fire a later one.
    Fire(Instant),
    /// One second of the delay elapsed. Redraw the numeral on the delay button.
    Refresh(Instant),
    /// Animate delay-toolbar to countdown-toolbar
    ToolbarFrame(Instant),
    /// Call the wait off and put the overlay back as it was.
    Cancel,
    /// Start dragging one output's countdown toolbar.
    DragStart(window::Id),
    /// Move the toolbar being dragged.
    DragMove(f32, f32),
    /// Stop dragging the toolbar and move its input zone to where it landed.
    DragEnd,
    /// The toolbar on this surface was laid out at this size.
    Measured(window::Id, f32, f32),
}

/// One output's countdown surface.
#[derive(Clone, Debug)]
pub struct CountdownWindow {
    pub id: window::Id,
    pub output_name: String,
    pub output_size: (f32, f32),
    /// Where the toolbar was moved to. `None` centers it along the bottom.
    pub pos: Option<(f32, f32)>,
    /// The toolbar's laid-out size, reported by its sensor. Until the first
    /// report, the size of the capture toolbar it takes over from.
    pub size: (f32, f32),
}

/// How far a delayed capture has left to wait.
#[derive(Clone, Debug)]
pub struct Countdown {
    pub started: Instant,
    pub duration: Duration,
    /// The capture toolbar's sections when the countdown took over. They close from there.
    pub outgoing: ToolbarProgress,
    pub outgoing_choice: Choice,
    pub toolbar_transition_finished: bool,
    pub windows: Vec<CountdownWindow>,
    pub dragging: Option<window::Id>,
    pub drag_offset: Option<(f32, f32)>,
    /// The countdown toolbar stays up through the capture, since its surface is capture-excluded.
    pub fired: bool,
}

impl Countdown {
    #[must_use]
    pub fn new(
        seconds: u32,
        windows: Vec<CountdownWindow>,
        outgoing: ToolbarProgress,
        outgoing_choice: Choice,
    ) -> Self {
        Self {
            started: Instant::now(),
            // zero doesn't make sense here
            duration: Duration::from_secs(u64::from(seconds.max(1))),
            outgoing,
            outgoing_choice,
            toolbar_transition_finished: false,
            windows,
            dragging: None,
            drag_offset: None,
            fired: false,
        }
    }

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
}

impl CountdownWindow {
    /// The toolbar's top-left corner on its output.
    #[must_use]
    pub fn origin(&self) -> (f32, f32) {
        crate::widget::toolbar::toolbar_origin(self.pos, self.size, self.output_size)
    }

    /// The area of the surface that accepts input: the toolbar plus a small margin.
    #[must_use]
    pub fn input_zone(&self) -> Rectangle {
        let (x, y) = self.origin();
        Rectangle {
            x: x - MARGIN,
            y: y - MARGIN,
            width: MARGIN.mul_add(2.0, self.size.0),
            height: MARGIN.mul_add(2.0, self.size.1),
        }
    }

    /// Move it, and keep it on screen.
    pub fn move_to(&mut self, pos: (f32, f32)) {
        self.pos = Some(crate::widget::toolbar::toolbar_origin(
            Some(pos),
            self.size,
            self.output_size,
        ));
    }

    /// Take the size the toolbar was laid out at. A moved toolbar is kept on screen at it.
    pub fn resize(&mut self, size: (f32, f32)) {
        self.size = size;
        if let Some(pos) = self.pos {
            self.move_to(pos);
        }
    }
}

/// What the toolbar sends back.
pub struct Handlers<Msg> {
    pub on_drag_start: Msg,
    pub on_drag_end: Msg,
    pub on_drag_move: Box<dyn Fn(f32, f32) -> Msg>,
    pub on_resize: Box<dyn Fn(f32, f32) -> Msg>,
    pub on_transition_noop: Msg,
    pub on_cancel: Msg,
}

/// The countdown toolbar, on the output that owns `id`.
#[must_use]
pub fn view<'a, Msg: Clone + 'static>(
    countdown: &Countdown,
    id: window::Id,
    now: Instant,
    handlers: Handlers<Msg>,
) -> cosmic::Element<'a, Msg> {
    use crate::widget::toolbar::{CountdownToolbar, Handoff, ToolbarMode};
    use cosmic::iced::Length;
    use cosmic::iced::widget::stack;

    let Some(window) = countdown.window(id) else {
        return cosmic::iced::widget::space()
            .width(Length::Fixed(1.0))
            .into();
    };

    let toolbar = crate::widget::toolbar::build_toolbar(ToolbarMode::Countdown(CountdownToolbar {
        handoff: Handoff {
            transition: countdown.toolbar_transition_progress(now),
            outgoing: countdown.outgoing,
            outgoing_choice: countdown.outgoing_choice.clone(),
            all_screens: countdown.windows.len() > 1,
        },
        started: countdown.started,
        duration: countdown.duration,
        remaining: countdown.remaining(now),
        on_drag_start: handlers.on_drag_start,
        on_drag_end: handlers.on_drag_end.clone(),
        on_transition_noop: handlers.on_transition_noop,
        on_cancel: handlers.on_cancel,
    }));
    let placed = crate::widget::toolbar::place_toolbar(
        toolbar,
        window.pos.map(|_| window.origin()),
        handlers.on_resize,
    );

    // Under the toolbar, so a drag continues past the input zone.
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

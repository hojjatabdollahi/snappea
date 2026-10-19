// SPDX-License-Identifier: GPL-3.0-only

//! The recording overlay: the capture-excluded chrome (region border and
//! toolbar) and the live-annotation surface that is recorded on purpose.

use crate::app::{Msg as AppMsg, OutputState};
use cosmic::Task;
use cosmic::cctk::sctk::shell::wlr_layer;
use cosmic::iced::core::layout::Limits;
use cosmic::iced::mouse;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{
    IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced::platform_specific::shell::commands::layer_surface::get_layer_surface;
use cosmic::iced::{Point, window};
use std::time::{Duration, Instant};
use wayland_client::protocol::wl_output::WlOutput;

/// A single annotation stroke with fade state
#[derive(Debug, Clone)]
pub struct AnnotationStroke {
    pub points: Vec<(f32, f32)>,
    /// When the stroke was finished, for the fade.
    pub completed_at: std::time::Instant,
    pub opacity: f32,
    pub color: viewer_tools::annotate::AnnotateColor,
    pub thickness: f32,
}

/// Layer shell namespace for the recording chrome. Must match cosmic-comp's
/// `CAPTURE_EXCLUDED_NAMESPACES` exactly. This keeps it out of the recording.
pub const CHROME_NAMESPACE: &str = "cosmic-screen-recorder-toolbar";

/// Layer shell namespace for live annotations. Captured on purpose.
pub const ANNOTATION_NAMESPACE: &str = "snappea-annotations";

/// Match the capture toolbar's section animation.
const RECORDING_TOOLBAR_ANIM: Duration = Duration::from_millis(180);

fn recording_toolbar_transition_progress(started: Instant, now: Instant) -> f32 {
    let linear =
        now.saturating_duration_since(started).as_secs_f32() / RECORDING_TOOLBAR_ANIM.as_secs_f32();
    let linear = linear.clamp(0.0, 1.0);
    // EaseOut cubic, matching the feel of ToolbarAnim's EaseOut sections.
    1.0 - (1.0 - linear).powi(3)
}

/// State for the recording indicator overlays
#[derive(Debug, Clone)]
pub struct RecordingIndicator {
    /// Window ID for the session overlay borrowed by the recording chrome.
    pub chrome_window_id: window::Id,
    /// Whether a layer surface currently exists for `chrome_window_id`
    pub chrome_surface_alive: bool,
    /// Window ID for the annotation layer surface
    pub video_live_annotation_window_id: window::Id,
    /// Whether a layer surface currently exists for `annotation_window_id`
    pub annotation_surface_alive: bool,
    /// Output hosting the recording chrome and annotation surface
    pub output: WlOutput,
    /// In logical pixels (for popup positioning)
    pub output_size: (f32, f32),
    /// Recording region in output-local logical coordinates
    pub region: (i32, i32, u32, u32),
    /// Whether the region border is drawn at all. Off, it never blinks on.
    pub region_border: bool,
    pub blink_visible: bool,
    pub annotations: Vec<AnnotationStroke>,
    pub current_stroke: Option<Vec<(f32, f32)>>,
    /// Where the pointer is over the annotation surface, while the pencil is
    /// active. The pencil icon is drawn there in place of the hidden pointer.
    pub pointer: Option<Point>,
    /// Whether annotation mode is active (overlay captures all input)
    pub annotation_mode: bool,
    pub pencil_color: viewer_tools::annotate::AnnotateColor,
    pub pencil_fade_duration: f32,
    pub pencil_thickness: f32,
    pub toolbar_pos: Option<(f32, f32)>,
    pub toolbar_size: (f32, f32),
    /// The capture toolbar's sections going away in the starting animation.
    pub outgoing: crate::capture::ToolbarProgress,
    pub toolbar_transition_started: Instant,
    /// Monotonic start time for the elapsed readout.
    pub recording_started: Instant,
    pub toolbar_transition_finished: bool,
    pub outgoing_choice: crate::geometry::Choice,
    pub toolbar_dragging: bool,
    /// Cursor offset within the toolbar when the drag began, resolved on the first motion.
    pub drag_offset: Option<(f32, f32)>,
    pub fade_popup_open: bool,
    pub thickness_popup_open: bool,
}

impl RecordingIndicator {
    /// The toolbar's top-left corner on the output.
    pub fn toolbar_origin(&self) -> (f32, f32) {
        crate::widget::toolbar::toolbar_origin(
            self.toolbar_pos,
            self.toolbar_size,
            self.output_size,
        )
    }

    /// The toolbar's rectangle, as laid out, with a little padding.
    fn toolbar_rect(&self) -> cosmic::iced::core::Rectangle {
        const MARGIN: f32 = 8.0;
        let (x, y) = self.toolbar_origin();
        cosmic::iced::core::Rectangle {
            x: x - MARGIN,
            y: y - MARGIN,
            width: MARGIN.mul_add(2.0, self.toolbar_size.0),
            height: MARGIN.mul_add(2.0, self.toolbar_size.1),
        }
    }

    /// Input zones for the chrome surface: the toolbar. Everything else on that
    /// surface is click-through.
    pub fn chrome_input_zones(&self) -> Vec<cosmic::iced::core::Rectangle> {
        // A popover draws past the toolbar's bounds, so the whole output takes input while one is open.
        if self.fade_popup_open || self.thickness_popup_open {
            return vec![cosmic::iced::core::Rectangle {
                x: 0.0,
                y: 0.0,
                width: self.output_size.0,
                height: self.output_size.1,
            }];
        }

        vec![self.toolbar_rect()]
    }

    #[must_use]
    fn toolbar_transition_progress(&self, now: Instant) -> f32 {
        if self.toolbar_transition_finished {
            return 1.0;
        }
        recording_toolbar_transition_progress(self.toolbar_transition_started, now)
    }

    /// Input zone for the annotation surface: the recording region while drawing
    /// is enabled, nothing otherwise.
    fn annotation_input_zones(&self) -> Vec<cosmic::iced::core::Rectangle> {
        if !self.annotation_mode {
            return Vec::new();
        }

        vec![cosmic::iced::core::Rectangle {
            x: self.region.0 as f32,
            y: self.region.1 as f32,
            width: self.region.2 as f32,
            height: self.region.3 as f32,
        }]
    }

    /// Push the input zones and keyboard interactivity to both surfaces. Both are
    /// surface state, so nothing is remapped.
    pub fn apply_state<Message: 'static>(&self) -> cosmic::Task<Message> {
        use cosmic::iced::platform_specific::shell::commands::layer_surface::{
            set_input_zone, set_keyboard_interactivity,
        };

        let mut tasks = Vec::new();
        if self.chrome_surface_alive {
            tasks.push(set_input_zone(
                self.chrome_window_id,
                Some(self.chrome_input_zones()),
            ));
            // Nothing on the chrome is typed into, and holding focus would take
            // it from whatever is being recorded.
            tasks.push(set_keyboard_interactivity(
                self.chrome_window_id,
                wlr_layer::KeyboardInteractivity::None,
            ));
        }
        if self.annotation_surface_alive {
            tasks.push(set_input_zone(
                self.video_live_annotation_window_id,
                Some(self.annotation_input_zones()),
            ));
            // Escape leaves annotation mode, so the surface takes the keyboard
            // only while there is a mode to leave.
            tasks.push(set_keyboard_interactivity(
                self.video_live_annotation_window_id,
                if self.annotation_mode {
                    wlr_layer::KeyboardInteractivity::OnDemand
                } else {
                    wlr_layer::KeyboardInteractivity::None
                },
            ));
        }
        cosmic::Task::batch(tasks)
    }

    /// Map the annotation surface once for the life of the recording, on the Top
    /// layer below the chrome.
    pub fn create_annotation_surface<Message: 'static>(&mut self) -> cosmic::Task<Message> {
        self.video_live_annotation_window_id = window::Id::unique();
        self.annotation_surface_alive = true;

        get_layer_surface(self.annotation_surface_settings())
    }

    fn annotation_surface_settings(&self) -> SctkLayerSurfaceSettings {
        SctkLayerSurfaceSettings {
            id: self.video_live_annotation_window_id,
            layer: wlr_layer::Layer::Top,
            keyboard_interactivity: wlr_layer::KeyboardInteractivity::None,
            input_zone: Some(self.annotation_input_zones()),
            anchor: wlr_layer::Anchor::all(),
            output: IcedOutput::Output(self.output.clone()),
            namespace: ANNOTATION_NAMESPACE.to_string(),
            size: Some((None, None)),
            exclusive_zone: -1,
            size_limits: Limits::NONE.min_height(1.0).min_width(1.0),
            ..Default::default()
        }
    }

    /// Close the captured annotation surface and every session overlay,
    /// including the one borrowed by the recording chrome.
    pub fn destroy_surfaces<Message: 'static>(
        &mut self,
        outputs: &[OutputState],
    ) -> cosmic::Task<Message> {
        use cosmic::iced::platform_specific::shell::commands::layer_surface::destroy_layer_surface;

        let mut tasks = Vec::new();
        if self.annotation_surface_alive {
            tasks.push(destroy_layer_surface(self.video_live_annotation_window_id));
            self.annotation_surface_alive = false;
        }
        self.chrome_surface_alive = false;
        tasks.extend(
            outputs
                .iter()
                .map(|output| destroy_layer_surface(output.id)),
        );
        cosmic::Task::batch(tasks)
    }
}

/// Shared canvas state for the two recording overlays.
#[derive(Default)]
struct OverlayState {
    cursor_position: cosmic::iced::core::Point,
}

const BORDER_WIDTH: f32 = 4.0;
/// Safety margin against rounding and scaling, so the stroke stays out of the crop.
/// TODO: this is not needed when cosmic-comp's PR to ignore this UI is merged.
const BORDER_MARGIN: f32 = 2.0;
fn border_sides(region: (i32, i32, u32, u32), output: (f32, f32)) -> Vec<(Point, Point)> {
    let gap = BORDER_WIDTH + BORDER_MARGIN;
    // The stroke straddles the path, so a side needs half a width to the edge
    // or its outer half gets clipped away.
    let inset = BORDER_WIDTH / 2.0;
    let (x, y, w, h) = region;
    let left = x as f32 - gap;
    let top = y as f32 - gap;
    let right = (x + w as i32) as f32 + gap;
    let bottom = (y + h as i32) as f32 + gap;
    // Ends clamped to the output, so a left-out neighbor leaves no dangling end.
    let clamp_x = |v: f32| v.clamp(0.0, output.0);
    let clamp_y = |v: f32| v.clamp(0.0, output.1);

    let mut sides = Vec::with_capacity(4);
    if left >= inset {
        sides.push((
            Point::new(left, clamp_y(top)),
            Point::new(left, clamp_y(bottom)),
        ));
    }
    if right <= output.0 - inset {
        sides.push((
            Point::new(right, clamp_y(top)),
            Point::new(right, clamp_y(bottom)),
        ));
    }
    if top >= inset {
        sides.push((
            Point::new(clamp_x(left), top),
            Point::new(clamp_x(right), top),
        ));
    }
    if bottom <= output.1 - inset {
        sides.push((
            Point::new(clamp_x(left), bottom),
            Point::new(clamp_x(right), bottom),
        ));
    }
    sides
}

/// Canvas for the recording chrome surface: draws the blinking region border and
/// carries the toolbar drag.
struct ChromeOverlay {
    region: (i32, i32, u32, u32),
    border_visible: bool,
    toolbar_dragging: bool,
}

impl cosmic::iced::widget::canvas::Program<AppMsg, cosmic::Theme, cosmic::Renderer>
    for ChromeOverlay
{
    type State = OverlayState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &cosmic::iced::Event,
        bounds: cosmic::iced::core::Rectangle,
        cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Option<cosmic::iced::widget::canvas::Action<AppMsg>> {
        use cosmic::iced::core::mouse::{Button, Event as MouseEvent};
        use cosmic::iced::widget::canvas;

        if let Some(pos) = cursor.position_in(bounds) {
            state.cursor_position = pos;
        }

        match event {
            canvas::Event::Mouse(MouseEvent::CursorMoved { position }) => {
                state.cursor_position = *position;

                if self.toolbar_dragging {
                    return Some(
                        canvas::Action::publish(AppMsg::Recording(Msg::ToolbarDragMove(
                            position.x, position.y,
                        )))
                        .and_capture(),
                    );
                }
            }
            canvas::Event::Mouse(MouseEvent::ButtonReleased(Button::Left))
                if self.toolbar_dragging =>
            {
                return Some(
                    canvas::Action::publish(AppMsg::Recording(Msg::ToolbarDragEnd)).and_capture(),
                );
            }
            _ => {}
        }

        // Let events pass through to the desktop otherwise
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: cosmic::iced::core::Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Vec<cosmic::iced::widget::canvas::Geometry> {
        use cosmic::iced::widget::canvas::{self, LineCap, Path, Stroke};

        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // The border sits just outside the region. Sides on the output's edge are left out.
        if self.border_visible {
            let path = Path::new(|b| {
                for (from, to) in border_sides(self.region, (bounds.width, bounds.height)) {
                    b.move_to(from);
                    b.line_to(to);
                }
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(cosmic::iced::core::Color::from_rgb(1.0, 0.0, 0.0))
                    .with_width(BORDER_WIDTH)
                    // Square caps fill the corner where two sides meet.
                    .with_line_cap(LineCap::Square),
            );
        }

        vec![frame.into_geometry()]
    }
}

/// Canvas for the annotation surface. Captured, unlike [`ChromeOverlay`].
struct AnnotationOverlay {
    annotations: Vec<AnnotationStroke>,
    current_stroke: Option<Vec<(f32, f32)>>,
    annotation_mode: bool,
    pencil_color: viewer_tools::annotate::AnnotateColor,
    pencil_thickness: f32,
}

impl cosmic::iced::widget::canvas::Program<AppMsg, cosmic::Theme, cosmic::Renderer>
    for AnnotationOverlay
{
    type State = OverlayState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &cosmic::iced::Event,
        bounds: cosmic::iced::core::Rectangle,
        cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Option<cosmic::iced::widget::canvas::Action<AppMsg>> {
        use cosmic::iced::core::mouse::{Button, Event as MouseEvent};
        use cosmic::iced::widget::canvas;

        if let Some(pos) = cursor.position_in(bounds) {
            state.cursor_position = pos;
        }

        match event {
            canvas::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
                if self.annotation_mode {
                    return Some(
                        canvas::Action::publish(AppMsg::Recording(Msg::Mouse(
                            cosmic::iced::mouse::Event::ButtonPressed(
                                cosmic::iced::mouse::Button::Left,
                            ),
                            state.cursor_position,
                        )))
                        .and_capture(),
                    );
                }
            }
            canvas::Event::Mouse(MouseEvent::CursorMoved { position }) => {
                state.cursor_position = *position;
                // Every move while the pencil is active, so the pencil icon follows
                // the pointer. Captured only mid-stroke.
                if self.annotation_mode {
                    let action = canvas::Action::publish(AppMsg::Recording(Msg::Mouse(
                        cosmic::iced::mouse::Event::CursorMoved {
                            position: *position,
                        },
                        *position,
                    )));
                    return Some(if self.current_stroke.is_some() {
                        action.and_capture()
                    } else {
                        action
                    });
                }
            }
            canvas::Event::Mouse(MouseEvent::CursorLeft) if self.annotation_mode => {
                return Some(canvas::Action::publish(AppMsg::Recording(Msg::Mouse(
                    cosmic::iced::mouse::Event::CursorLeft,
                    state.cursor_position,
                ))));
            }
            canvas::Event::Mouse(MouseEvent::ButtonReleased(Button::Left))
                if self.current_stroke.is_some() =>
            {
                return Some(
                    canvas::Action::publish(AppMsg::Recording(Msg::Mouse(
                        cosmic::iced::mouse::Event::ButtonReleased(
                            cosmic::iced::mouse::Button::Left,
                        ),
                        state.cursor_position,
                    )))
                    .and_capture(),
                );
            }
            _ => {}
        }

        // Let events pass through when not drawing
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: cosmic::iced::core::Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Vec<cosmic::iced::widget::canvas::Geometry> {
        use cosmic::iced::widget::canvas::{self, Path, Stroke};

        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // Completed strokes, each with its own color, thickness and fade opacity
        for stroke in &self.annotations {
            if stroke.points.len() < 2 {
                continue;
            }

            let path = Path::new(|builder| {
                builder.move_to(cosmic::iced::core::Point::new(
                    stroke.points[0].0,
                    stroke.points[0].1,
                ));
                for point in &stroke.points[1..] {
                    builder.line_to(cosmic::iced::core::Point::new(point.0, point.1));
                }
            });

            let color = cosmic::iced::core::Color {
                a: stroke.opacity,
                ..stroke.color.0
            };

            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(color)
                    .with_width(stroke.thickness)
                    .with_line_cap(canvas::LineCap::Round)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }

        // Stroke being drawn right now: full opacity, current settings
        if let Some(points) = &self.current_stroke
            && points.len() >= 2
        {
            let path = Path::new(|builder| {
                builder.move_to(cosmic::iced::core::Point::new(points[0].0, points[0].1));
                for point in &points[1..] {
                    builder.line_to(cosmic::iced::core::Point::new(point.0, point.1));
                }
            });

            let color = cosmic::iced::core::Color {
                a: 1.0,
                ..self.pencil_color.0
            };

            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(color)
                    .with_width(self.pencil_thickness)
                    .with_line_cap(canvas::LineCap::Round)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: cosmic::iced::core::Rectangle,
        cursor: cosmic::iced::core::mouse::Cursor,
    ) -> cosmic::iced::core::mouse::Interaction {
        if self.annotation_mode && cursor.is_over(bounds) {
            cosmic::iced::core::mouse::Interaction::Hidden
        } else {
            cosmic::iced::core::mouse::Interaction::default()
        }
    }
}

/// Render the live annotations drawn during a recording.
pub fn render_recording_annotations(
    indicator: &RecordingIndicator,
) -> cosmic::Element<'static, AppMsg> {
    use cosmic::iced::core::Length;

    let canvas = cosmic::iced::widget::canvas::Canvas::new(AnnotationOverlay {
        annotations: indicator.annotations.clone(),
        current_stroke: indicator.current_stroke.clone(),
        annotation_mode: indicator.annotation_mode,
        pencil_color: indicator.pencil_color,
        pencil_thickness: indicator.pencil_thickness,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    // The pencil icon stands in for the hidden pointer, its tip on the point that
    // draws. Recorded with the strokes.
    let Some(pointer) = indicator.pointer.filter(|_| indicator.annotation_mode) else {
        return canvas.into();
    };
    const PENCIL_ICON: u16 = 24;
    // Where the tip sits in `pencil-symbolic.svg`, as fractions of its 16px box.
    const TIP: (f32, f32) = (1.1 / 16.0, 13.9 / 16.0);
    let size = f32::from(PENCIL_ICON);
    let top_left = Point::new(
        TIP.0.mul_add(-size, pointer.x),
        TIP.1.mul_add(-size, pointer.y),
    );
    let icon = crate::widget::toolbar::toolbar_icon_colored(
        crate::config::ShapeTool::Pen.icon_name(),
        PENCIL_ICON,
        indicator.pencil_color.0,
    )
    .width(Length::Fixed(f32::from(PENCIL_ICON)))
    .height(Length::Fixed(f32::from(PENCIL_ICON)));
    let pencil_layer = cosmic::iced::widget::column![
        cosmic::iced::widget::space().height(Length::Fixed(top_left.y.max(0.0))),
        cosmic::iced::widget::row![
            cosmic::iced::widget::space().width(Length::Fixed(top_left.x.max(0.0))),
            icon,
            cosmic::iced::widget::space().width(Length::Fill),
        ],
        cosmic::iced::widget::space().height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    cosmic::iced::widget::stack![canvas, pencil_layer]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Render the recording chrome, which cosmic-comp keeps out of captures.
pub fn render_recording_chrome(indicator: &RecordingIndicator) -> cosmic::Element<'static, AppMsg> {
    use crate::widget::toolbar::{Handoff, RecordingToolbar, ToolbarMode};
    use cosmic::iced::core::Length;
    use cosmic::iced::widget::stack;

    let canvas_layer = cosmic::iced::widget::canvas::Canvas::new(ChromeOverlay {
        region: indicator.region,
        border_visible: indicator.blink_visible,
        toolbar_dragging: indicator.toolbar_dragging,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let now = Instant::now();
    let toolbar = crate::widget::toolbar::build_toolbar(ToolbarMode::Recording(RecordingToolbar {
        handoff: Handoff {
            transition: indicator.toolbar_transition_progress(now),
            outgoing: indicator.outgoing,
            outgoing_choice: indicator.outgoing_choice.clone(),
            // Video mode has no all-screens button.
            all_screens: false,
        },
        elapsed: now.saturating_duration_since(indicator.recording_started),
        pencil_active: indicator.annotation_mode,
        color: indicator.pencil_color,
        fade_duration: indicator.pencil_fade_duration,
        fade_popup_open: indicator.fade_popup_open,
        thickness: indicator.pencil_thickness,
        thickness_popup_open: indicator.thickness_popup_open,
        on_drag_start: AppMsg::Recording(Msg::ToolbarDragStart),
        on_drag_end: AppMsg::Recording(Msg::ToolbarDragEnd),
        on_transition_noop: AppMsg::Ignore,
        on_pencil_toggle: AppMsg::Screenshot(crate::capture::msg::Msg::Action(
            crate::capture::msg::ActionMsg::ToggleRecordingAnnotation,
        )),
        on_stop: AppMsg::Screenshot(crate::capture::msg::Msg::Action(
            crate::capture::msg::ActionMsg::StopRecording,
        )),
        // Its own dropdown.
        on_fade_popup: AppMsg::Recording(Msg::FadePopup),
        on_thickness_popup: AppMsg::Recording(Msg::ThicknessPopup),
        on_select_thickness: Box::new(|t| {
            AppMsg::Screenshot(crate::capture::msg::Msg::Tool(
                crate::capture::msg::ToolMsg::SetPencilThickness(t),
            ))
        }),
        on_select_color: Box::new(|c| {
            AppMsg::Screenshot(crate::capture::msg::Msg::Tool(
                crate::capture::msg::ToolMsg::SetPencilColor(c),
            ))
        }),
        on_select_fade: Box::new(|d| {
            AppMsg::Screenshot(crate::capture::msg::Msg::Tool(
                crate::capture::msg::ToolMsg::SetPencilFadeDuration(d),
            ))
        }),
    }));
    let toolbar_layer = crate::widget::toolbar::place_toolbar(
        toolbar,
        indicator.toolbar_pos.map(|_| indicator.toolbar_origin()),
        |w, h| AppMsg::Recording(Msg::ToolbarMeasured(w, h)),
    );

    stack![canvas_layer, toolbar_layer]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// What the recording overlay reacts to. Wrapped in `app::Msg::Recording`.
#[derive(Debug, Clone)]
pub enum Msg {
    Blink,
    /// Advance the video-toolbar to recording-toolbar morph.
    ToolbarFrame(Instant),
    AnnotationFade,
    Mouse(mouse::Event, Point),
    FadePopup,
    ThicknessPopup,
    ToolbarDragStart,
    ToolbarDragMove(f32, f32),
    ToolbarDragEnd,
    ToolbarMeasured(f32, f32),
}

impl RecordingIndicator {
    /// Drive the overlay. Returns whatever surface work the change needs.
    pub fn update<M: 'static>(&mut self, msg: Msg) -> Task<M> {
        match msg {
            Msg::Blink => {
                self.blink_visible = self.region_border && !self.blink_visible;
                Task::none()
            }
            Msg::ToolbarFrame(now) => {
                // Stops the frame subscription. The input zone follows the
                // toolbar's size through `ToolbarMeasured`.
                if now.saturating_duration_since(self.toolbar_transition_started)
                    >= RECORDING_TOOLBAR_ANIM
                {
                    self.toolbar_transition_finished = true;
                }
                Task::none()
            }
            Msg::AnnotationFade => {
                // Strokes stay opaque for 80% of the fade duration, then fade out linearly.
                let hold = self.pencil_fade_duration * 0.8;
                let fade = self.pencil_fade_duration * 0.2;
                for stroke in &mut self.annotations {
                    let elapsed = stroke.completed_at.elapsed().as_secs_f32();
                    stroke.opacity = if elapsed <= hold {
                        1.0
                    } else {
                        1.0 - ((elapsed - hold) / fade).min(1.0)
                    };
                }
                self.annotations.retain(|s| s.opacity > 0.0);
                Task::none()
            }
            Msg::Mouse(event, position) => {
                self.pointer = if matches!(event, mouse::Event::CursorLeft) {
                    None
                } else {
                    Some(position)
                };
                let can_draw = self.annotation_mode;
                match event {
                    mouse::Event::ButtonPressed(mouse::Button::Left) => {
                        if can_draw {
                            self.current_stroke = Some(vec![(position.x, position.y)]);
                        }
                    }
                    mouse::Event::CursorMoved { .. } => {
                        if can_draw && let Some(stroke) = &mut self.current_stroke {
                            stroke.push((position.x, position.y));
                        }
                    }
                    mouse::Event::ButtonReleased(mouse::Button::Left) => {
                        if let Some(points) = self.current_stroke.take()
                            && points.len() > 1
                        {
                            self.annotations.push(AnnotationStroke {
                                points,
                                completed_at: Instant::now(),
                                opacity: 1.0,
                                color: self.pencil_color,
                                thickness: self.pencil_thickness,
                            });
                        }
                    }
                    _ => {}
                }
                Task::none()
            }
            Msg::FadePopup | Msg::ThicknessPopup => {
                if matches!(msg, Msg::FadePopup) {
                    self.fade_popup_open = !self.fade_popup_open;
                    self.thickness_popup_open = false;
                } else {
                    self.thickness_popup_open = !self.thickness_popup_open;
                    self.fade_popup_open = false;
                }
                // The input zone has to grow to cover the panel. A zone is
                // surface state, not a surface, so this is a request.
                self.apply_state()
            }
            Msg::ToolbarDragStart => {
                self.toolbar_dragging = true;
                // Resolved on the first move.
                self.drag_offset = None;
                Task::none()
            }
            Msg::ToolbarDragMove(x, y) => {
                if self.toolbar_dragging {
                    let (left, top) = self.toolbar_origin();
                    let (dx, dy) = *self.drag_offset.get_or_insert((x - left, y - top));
                    self.toolbar_pos = Some(crate::widget::toolbar::toolbar_origin(
                        Some((x - dx, y - dy)),
                        self.toolbar_size,
                        self.output_size,
                    ));
                }
                Task::none()
            }
            Msg::ToolbarDragEnd => {
                self.toolbar_dragging = false;
                // The moved toolbar just needs its zone moved with it.
                self.apply_state()
            }
            Msg::ToolbarMeasured(w, h) => {
                self.toolbar_size = (w, h);
                // A moved toolbar is kept on screen at its new size.
                self.toolbar_pos = self.toolbar_pos.map(|_| self.toolbar_origin());
                // Mid-drag the zone follows on release.
                if self.toolbar_dragging {
                    return Task::none();
                }
                self.apply_state()
            }
        }
    }
}

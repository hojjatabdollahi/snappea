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
    /// Points in the stroke (output-local coordinates)
    pub points: Vec<(f32, f32)>,
    /// When this stroke was completed (for fade calculation)
    pub completed_at: Option<std::time::Instant>,
    /// Opacity (1.0 = fully visible, 0.0 = invisible)
    pub opacity: f32,
    /// Color of this stroke (RGB, 0.0-1.0)
    pub color: viewer_tools::annotate::AnnotateColor,
    /// Line thickness in pixels
    pub thickness: f32,
}

/// Layer shell namespace for the recording chrome. Must match cosmic-comp's
/// `CAPTURE_EXCLUDED_NAMESPACES` exactly. This keeps it out of the recording.
pub const CHROME_NAMESPACE: &str = "cosmic-screen-recorder-toolbar";

/// Layer shell namespace for live annotations. Captured on purpose.
pub const ANNOTATION_NAMESPACE: &str = "cosmic-x-annotations";

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
    pub annotation_window_id: window::Id,
    /// Whether a layer surface currently exists for `annotation_window_id`
    pub annotation_surface_alive: bool,
    /// Output hosting the recording chrome and annotation surface
    pub output: WlOutput,
    /// Output size in logical pixels (for popup positioning)
    pub output_size: (f32, f32),
    /// Recording region in output-local logical coordinates
    pub region: (i32, i32, u32, u32),
    /// Whether the region border is drawn at all. Off, it never blinks on.
    pub region_border: bool,
    /// Current blink state (true = visible border)
    pub blink_visible: bool,
    /// Completed annotation strokes
    pub annotations: Vec<AnnotationStroke>,
    /// Current stroke being drawn (if any)
    pub current_stroke: Option<Vec<(f32, f32)>>,
    /// Where the pointer is over the annotation surface, while the pencil is
    /// active. The pencil icon is drawn there in place of the hidden pointer.
    pub pointer: Option<Point>,
    /// Whether annotation mode is active (overlay captures all input)
    pub annotation_mode: bool,
    /// Pencil color (RGB, 0.0-1.0)
    pub pencil_color: viewer_tools::annotate::AnnotateColor,
    /// Duration in seconds before pencil strokes fade away
    pub pencil_fade_duration: f32,
    /// Pencil line thickness in pixels
    pub pencil_thickness: f32,
    /// Toolbar bounds from main UI (output-local coords)
    pub toolbar_bounds: Option<cosmic::iced::core::Rectangle>,
    /// Toolbar position (top-left corner)
    pub toolbar_pos: (f32, f32),
    /// Position and bounds occupied by the video toolbar on the last frame
    /// before recording began.
    pub toolbar_transition_from_pos: (f32, f32),
    pub toolbar_transition_bounds: cosmic::iced::core::Rectangle,
    pub toolbar_transition_started: Instant,
    /// Monotonic start time for the elapsed readout.
    pub recording_started: Instant,
    pub toolbar_transition_finished: bool,
    pub outgoing_choice: crate::geometry::Choice,
    /// Whether toolbar is being dragged
    pub toolbar_dragging: bool,
    /// Drag offset from toolbar top-left when drag started
    pub drag_offset: (f32, f32),
    /// Whether pencil popup is open
    /// Fade-duration dropdown on the recording toolbar
    pub fade_popup_open: bool,
    /// Thickness dropdown on the recording toolbar
    pub thickness_popup_open: bool,
    pub pencil_popup_open: bool,
    /// Pencil popup bounds for input zone calculation
    pub pencil_popup_bounds: Option<cosmic::iced::core::Rectangle>,
}

impl RecordingIndicator {
    /// Input zones for the chrome surface: the toolbar, plus the pencil popup
    /// when it is open. Everything else on that surface is click-through.
    /// The toolbar's rectangle, sized from what is drawn, with a little padding.
    fn toolbar_rect(&self) -> cosmic::iced::core::Rectangle {
        const MARGIN: f32 = 8.0;
        let (w, h) = crate::widget::toolbar::recording_toolbar_size(self.annotation_mode);
        cosmic::iced::core::Rectangle {
            x: self.toolbar_pos.0 - MARGIN,
            y: self.toolbar_pos.1 - MARGIN,
            width: MARGIN.mul_add(2.0, w),
            height: MARGIN.mul_add(2.0, h),
        }
    }

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

        let toolbar = self.toolbar_rect();
        if self.toolbar_transition_finished {
            return vec![toolbar];
        }

        // Keep both ends of the morph live as surface state catches up with the
        // changing widget width. The departing controls themselves are inert.
        let from = self.toolbar_transition_bounds;
        let left = from.x.min(toolbar.x);
        let top = from.y.min(toolbar.y);
        let right = (from.x + from.width).max(toolbar.x + toolbar.width);
        let bottom = (from.y + from.height).max(toolbar.y + toolbar.height);
        vec![cosmic::iced::core::Rectangle {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }]
    }

    #[must_use]
    fn toolbar_transition_progress(&self, now: Instant) -> f32 {
        if self.toolbar_transition_finished {
            return 1.0;
        }
        recording_toolbar_transition_progress(self.toolbar_transition_started, now)
    }

    #[must_use]
    fn displayed_toolbar_pos(&self, now: Instant) -> (f32, f32) {
        let progress = self.toolbar_transition_progress(now);
        (
            (self.toolbar_pos.0 - self.toolbar_transition_from_pos.0)
                .mul_add(progress, self.toolbar_transition_from_pos.0),
            (self.toolbar_pos.1 - self.toolbar_transition_from_pos.1)
                .mul_add(progress, self.toolbar_transition_from_pos.1),
        )
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
                self.annotation_window_id,
                Some(self.annotation_input_zones()),
            ));
            // Escape leaves annotation mode, so the surface takes the keyboard
            // only while there is a mode to leave.
            tasks.push(set_keyboard_interactivity(
                self.annotation_window_id,
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
        self.annotation_window_id = window::Id::unique();
        self.annotation_surface_alive = true;

        get_layer_surface(self.annotation_surface_settings())
    }

    fn annotation_surface_settings(&self) -> SctkLayerSurfaceSettings {
        SctkLayerSurfaceSettings {
            id: self.annotation_window_id,
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
            tasks.push(destroy_layer_surface(self.annotation_window_id));
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

#[cfg(test)]
mod recording_toolbar_transition_tests {
    use super::*;

    #[test]
    fn transition_open_to_closed() {
        let started = Instant::now();
        assert_eq!(recording_toolbar_transition_progress(started, started), 0.0);
        assert_eq!(
            recording_toolbar_transition_progress(started, started + RECORDING_TOOLBAR_ANIM),
            1.0
        );
    }

    #[test]
    fn transition_eases_out() {
        let started = Instant::now();
        let halfway =
            recording_toolbar_transition_progress(started, started + RECORDING_TOOLBAR_ANIM / 2);
        assert!(halfway > 0.5 && halfway < 1.0);
    }
}

/// Shared canvas state for the two recording overlays.
#[derive(Default)]
struct OverlayState {
    cursor_position: cosmic::iced::core::Point,
}

/// Canvas for the recording chrome surface: draws the blinking region border and
/// carries the toolbar drag and click-outside-to-close interactions.
struct ChromeOverlay {
    region: (i32, i32, u32, u32),
    border_visible: bool,
    pencil_popup_open: bool,
    pencil_popup_bounds: Option<cosmic::iced::core::Rectangle>,
    toolbar_bounds: Option<cosmic::iced::core::Rectangle>,
    toolbar_dragging: bool,
}

/// Publish `Close` for the pencil popup when a click lands outside it and the toolbar.
fn close_popup_on_click_outside(
    popup_open: bool,
    popup_bounds: Option<cosmic::iced::core::Rectangle>,
    toolbar_bounds: Option<cosmic::iced::core::Rectangle>,
    cursor: cosmic::iced::core::mouse::Cursor,
) -> Option<cosmic::iced::widget::canvas::Action<AppMsg>> {
    use cosmic::iced::widget::canvas;

    if !popup_open {
        return None;
    }
    let cursor_pos = cursor.position()?;

    let in_popup = popup_bounds.is_some_and(|b| b.contains(cursor_pos));
    let in_toolbar = toolbar_bounds.is_some_and(|b| b.contains(cursor_pos));
    if in_popup || in_toolbar {
        return None;
    }

    Some(
        canvas::Action::publish(AppMsg::Screenshot(crate::capture::msg::Msg::Tool(
            crate::capture::msg::ToolMsg::PencilPopup(crate::capture::msg::ToolPopupAction::Close),
        )))
        .and_capture(),
    )
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
            canvas::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
                return close_popup_on_click_outside(
                    self.pencil_popup_open,
                    self.pencil_popup_bounds,
                    self.toolbar_bounds,
                    cursor,
                );
            }
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
        use cosmic::iced::widget::canvas::{self, Path, Stroke};

        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // The border sits just outside the region, clamped inside the output at its edges.
        if self.border_visible {
            let border_width = 4.0;
            // Add safety margin to account for rounding/scaling errors
            let margin = 2.0;
            // The stroke straddles the path, so keep the path half a width inside
            // the canvas or the outer half gets clipped away.
            let inset = border_width / 2.0;

            let left = (self.region.0 as f32 - border_width - margin).max(inset);
            let top = (self.region.1 as f32 - border_width - margin).max(inset);
            let right = ((self.region.0 + self.region.2 as i32) as f32 + border_width + margin)
                .min(bounds.width - inset);
            let bottom = ((self.region.1 + self.region.3 as i32) as f32 + border_width + margin)
                .min(bounds.height - inset);

            if right > left && bottom > top {
                let path = Path::rectangle(
                    cosmic::iced::core::Point::new(left, top),
                    cosmic::iced::core::Size::new(right - left, bottom - top),
                );

                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(cosmic::iced::core::Color::from_rgb(1.0, 0.0, 0.0))
                        .with_width(border_width),
                );
            }
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
    pencil_popup_open: bool,
    pencil_popup_bounds: Option<cosmic::iced::core::Rectangle>,
    toolbar_bounds: Option<cosmic::iced::core::Rectangle>,
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
                if let Some(action) = close_popup_on_click_outside(
                    self.pencil_popup_open,
                    self.pencil_popup_bounds,
                    self.toolbar_bounds,
                    cursor,
                ) {
                    return Some(action);
                }

                // Start drawing (unless the popup is handling the click)
                if self.annotation_mode && !self.pencil_popup_open {
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
                // the pointer. Captured only mid-stroke, and never while the popup
                // is open.
                if self.annotation_mode && !self.pencil_popup_open {
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
            // Don't capture release if popup is open (even mid-stroke)
            canvas::Event::Mouse(MouseEvent::ButtonReleased(Button::Left))
                if self.current_stroke.is_some() && !self.pencil_popup_open =>
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
        pencil_popup_open: indicator.pencil_popup_open,
        pencil_popup_bounds: indicator.pencil_popup_bounds,
        toolbar_bounds: indicator.toolbar_bounds,
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
    use cosmic::iced::core::Length;

    let annotation_mode = indicator.annotation_mode;
    let pencil_color = indicator.pencil_color;
    let pencil_thickness = indicator.pencil_thickness;
    let pencil_popup_open = indicator.pencil_popup_open;
    let pencil_fade_duration = indicator.pencil_fade_duration;

    let canvas_layer = cosmic::iced::widget::canvas::Canvas::new(ChromeOverlay {
        region: indicator.region,
        border_visible: indicator.blink_visible,
        pencil_popup_open,
        pencil_popup_bounds: indicator.pencil_popup_bounds,
        toolbar_bounds: indicator.toolbar_bounds,
        toolbar_dragging: indicator.toolbar_dragging,
    })
    .width(Length::Fill)
    .height(Length::Fill);
    // Add toolbar with stop and pencil toggle buttons

    use cosmic::iced::widget::row;

    let now = Instant::now();
    let toolbar_pos = indicator.displayed_toolbar_pos(now);
    let toolbar_transition = indicator.toolbar_transition_progress(now);

    // The recording toolbar: grip, drawing tools, stop.
    let toolbar_content: cosmic::Element<'static, AppMsg> =
        crate::widget::toolbar::build_recording_toolbar(crate::widget::toolbar::RecordingToolbar {
            transition: toolbar_transition,
            outgoing_choice: indicator.outgoing_choice.clone(),
            elapsed: now.saturating_duration_since(indicator.recording_started),
            pencil_active: annotation_mode,
            color: pencil_color,
            fade_duration: pencil_fade_duration,
            fade_popup_open: indicator.fade_popup_open,
            thickness: pencil_thickness,
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
        });

    let pencil_popup: Option<cosmic::Element<'static, AppMsg>> = None;

    // Stack canvas, toolbar, and popup with proper vertical positioning
    use cosmic::iced::widget::{column, stack};

    // Toolbar styled container
    // build_recording_toolbar draws the panel itself.
    let toolbar_with_bg = toolbar_content;

    // Toolbar layer: positioned using toolbar_pos for drag support
    // Use Space widgets to position toolbar at the desired location
    // Create horizontal spacer to position toolbar
    let left_space = cosmic::iced::widget::space().width(Length::Fixed(toolbar_pos.0.max(0.0)));
    let top_space = cosmic::iced::widget::space().height(Length::Fixed(toolbar_pos.1.max(0.0)));

    // Build the positioned toolbar
    let toolbar_row = row![
        left_space,
        toolbar_with_bg,
        cosmic::iced::widget::space().width(Length::Fill)
    ]
    .width(Length::Fill);

    let toolbar_layer = column![
        top_space,
        toolbar_row,
        cosmic::iced::widget::space().height(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    // Build stack with popup above toolbar if open
    if let Some(popup) = pencil_popup {
        // Popup layer: positioned relative to toolbar, above or below based on space
        let popup_gap = 16.0_f32; // Gap between popup and toolbar
        let popup_height = 380.0_f32; // Approximate popup height (must match screenshot/mod.rs)
        let toolbar_height = 72.0_f32; // Toolbar height including padding
        let output_size = indicator.output_size;

        // Determine if popup should appear above or below toolbar
        let space_above = toolbar_pos.1;
        let space_below = output_size.1 - toolbar_pos.1 - toolbar_height;

        let popup_y = if space_above >= popup_height + popup_gap {
            // Place above toolbar
            toolbar_pos.1 - popup_gap - popup_height
        } else if space_below >= popup_height + popup_gap {
            // Place below toolbar
            toolbar_pos.1 + toolbar_height + popup_gap
        } else {
            // Not enough space either way, place above and let it clip at top
            (toolbar_pos.1 - popup_gap - popup_height).max(0.0)
        };

        let popup_left_space =
            cosmic::iced::widget::space().width(Length::Fixed(toolbar_pos.0.max(0.0)));
        let popup_top_space = cosmic::iced::widget::space().height(Length::Fixed(popup_y.max(0.0)));

        let popup_row = row![
            popup_left_space,
            popup,
            cosmic::iced::widget::space().width(Length::Fill)
        ]
        .width(Length::Fill);

        let popup_layer = column![
            popup_top_space,
            popup_row,
            cosmic::iced::widget::space().height(Length::Fill)
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        stack![canvas_layer, toolbar_layer, popup_layer]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        stack![canvas_layer, toolbar_layer]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// What the recording overlay reacts to. Wrapped in `app::Msg::Recording`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// Toggle the region border's blink state.
    Blink,
    /// Advance the video-toolbar to recording-toolbar morph.
    ToolbarFrame(Instant),
    /// Fade completed pencil strokes. Sent periodically.
    AnnotationFade,
    /// Mouse event on the annotation surface.
    Mouse(mouse::Event, Point),
    /// `annotation_mode` changed. Rebuild the surfaces with the new input zones.
    ToggleAnnotationMode,
    /// Toggle the toolbar's fade dropdown.
    FadePopup,
    /// Toggle the toolbar's thickness dropdown.
    ThicknessPopup,
    /// Start dragging the toolbar. The grab offset is taken on the first move.
    ToolbarDragStart,
    /// Move the toolbar while dragging.
    ToolbarDragMove(f32, f32),
    /// Stop dragging the toolbar.
    ToolbarDragEnd,
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
                if !self.toolbar_transition_finished
                    && now.saturating_duration_since(self.toolbar_transition_started)
                        >= RECORDING_TOOLBAR_ANIM
                {
                    self.toolbar_transition_finished = true;
                    // The transition input zone covers both toolbar widths. Once
                    // the morph lands, trim it to the recording toolbar itself.
                    return self.apply_state();
                }
                Task::none()
            }
            Msg::AnnotationFade => {
                // Strokes stay opaque for 80% of the fade duration, then fade out linearly.
                let hold = self.pencil_fade_duration * 0.8;
                let fade = self.pencil_fade_duration * 0.2;
                for stroke in &mut self.annotations {
                    if let Some(completed_at) = stroke.completed_at {
                        let elapsed = completed_at.elapsed().as_secs_f32();
                        stroke.opacity = if elapsed <= hold {
                            1.0
                        } else {
                            1.0 - ((elapsed - hold) / fade).min(1.0)
                        };
                    }
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
                                completed_at: Some(Instant::now()),
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
            Msg::ToggleAnnotationMode => self.apply_state(),
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
                if !self.toolbar_transition_finished {
                    // A drag sets the toolbar's position. Finish the morph at that position.
                    self.toolbar_pos = self.displayed_toolbar_pos(Instant::now());
                    self.toolbar_transition_finished = true;
                }
                self.toolbar_dragging = true;
                // Resolved on the first move.
                self.drag_offset = (0.0, 0.0);
                Task::none()
            }
            Msg::ToolbarDragMove(x, y) => {
                if self.toolbar_dragging {
                    if self.drag_offset == (0.0, 0.0) {
                        self.drag_offset = (x - self.toolbar_pos.0, y - self.toolbar_pos.1);
                    }
                    self.toolbar_pos = (x - self.drag_offset.0, y - self.drag_offset.1);
                }
                Task::none()
            }
            Msg::ToolbarDragEnd => {
                self.toolbar_dragging = false;
                // One source for the toolbar's rectangle.
                self.toolbar_bounds = Some(self.toolbar_rect());
                // The moved toolbar just needs its zone moved with it.
                self.apply_state()
            }
        }
    }
}

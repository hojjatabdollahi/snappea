use crate::core::portal::{DBUS_NAME, DBUS_PATH};
use crate::screenshot;
use crate::session::messages;
use cosmic::Task;
use cosmic::iced_core::event::wayland::OutputEvent;
use cosmic::{
    app,
    iced::window,
    iced_futures::{Subscription, event::listen_with},
};
use futures::SinkExt;
use std::any::TypeId;
use wayland_client::protocol::wl_output::WlOutput;

pub(crate) fn run() -> cosmic::iced::Result {
    let settings = cosmic::app::Settings::default()
        .no_main_window(true)
        .exit_on_close(false);
    cosmic::app::run::<App>(settings, ())
}

pub struct App {
    pub core: app::Core,
    pub tx: Option<tokio::sync::mpsc::Sender<screenshot::Event>>,
    pub screenshot_args: Option<screenshot::Args>,
    pub location_options: Vec<String>,
    pub wayland_helper: crate::wayland::WaylandHelper,
    pub outputs: Vec<OutputState>,
    pub active_output: Option<WlOutput>,
    /// Recording indicator overlay state
    pub recording_indicator: Option<RecordingIndicator>,
}

/// A single annotation stroke with fade state
#[derive(Debug, Clone)]
pub struct AnnotationStroke {
    /// Points in the stroke (output-local coordinates)
    pub points: Vec<(f32, f32)>,
    /// When this stroke was completed (for fade calculation)
    pub completed_at: Option<std::time::Instant>,
    /// Opacity (1.0 = fully visible, 0.0 = invisible)
    pub opacity: f32,
}

/// State for the recording indicator overlay
#[derive(Debug, Clone)]
pub struct RecordingIndicator {
    /// Window ID for the layer surface
    pub window_id: window::Id,
    /// Output name where recording is happening
    pub output_name: String,
    /// Output for recreating the surface
    pub output: WlOutput,
    /// Recording region in output-local logical coordinates
    pub region: (i32, i32, u32, u32),
    /// Current blink state (true = visible border)
    pub blink_visible: bool,
    /// Completed annotation strokes
    pub annotations: Vec<AnnotationStroke>,
    /// Current stroke being drawn (if any)
    pub current_stroke: Option<Vec<(f32, f32)>>,
    /// Whether Super key is pressed
    pub super_pressed: bool,
    /// Whether Ctrl key is pressed
    pub ctrl_pressed: bool,
    /// Whether annotation mode is active (overlay captures all input)
    pub annotation_mode: bool,
    /// Toolbar position (top-left corner)
    pub toolbar_pos: (f32, f32),
    /// Whether toolbar is being dragged
    pub toolbar_dragging: bool,
    /// Drag offset from toolbar top-left when drag started
    pub drag_offset: (f32, f32),
}

#[derive(Debug, Clone)]
pub struct OutputState {
    pub output: WlOutput,
    pub id: window::Id,
    pub name: String,
    pub logical_size: (u32, u32),
    pub logical_pos: (i32, i32),
    pub scale_factor: i32,
    pub has_pointer: bool,
    pub bg_source: Option<cosmic_bg_config::Source>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Msg {
    Screenshot(messages::Msg),
    Portal(screenshot::Event),
    Output(OutputEvent, WlOutput),
    Keyboard(cosmic::iced::keyboard::Event),
    /// Toggle recording indicator blink state
    RecordingBlink,
    /// Recording has stopped
    RecordingStopped,
    /// Update annotation fade (called periodically)
    AnnotationFade,
    /// Mouse event on recording indicator
    IndicatorMouse(cosmic::iced::mouse::Event, cosmic::iced::Point),
    /// Keyboard modifiers changed on indicator
    IndicatorModifiers(cosmic::iced::keyboard::Modifiers),
    /// Toggle annotation mode (switch between click-through and drawing mode)
    ToggleAnnotationMode,
    /// Stop the recording
    StopRecording,
    /// Start dragging the toolbar
    ToolbarDragStart(f32, f32),
    /// Update toolbar position while dragging
    ToolbarDragMove(f32, f32),
    /// Stop dragging the toolbar
    ToolbarDragEnd,
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;

    type Flags = ();

    type Message = Msg;

    const APP_ID: &'static str = "io.github.hojjatabdollahi.snappea";

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn init(
        core: app::Core,
        _flags: Self::Flags,
    ) -> (Self, cosmic::iced::Task<cosmic::Action<Self::Message>>) {
        let wayland_conn = wayland_client::Connection::connect_to_env().unwrap();
        let wayland_helper = crate::wayland::WaylandHelper::new(wayland_conn);
        (
            Self {
                core,
                screenshot_args: Default::default(),
                location_options: Vec::new(),
                outputs: Default::default(),
                active_output: Default::default(),
                wayland_helper,
                tx: None,
                recording_indicator: None,
            },
            cosmic::iced::Task::none(),
        )
    }

    fn view(&self) -> cosmic::Element<'_, Self::Message> {
        unimplemented!()
    }

    fn view_window(&self, id: window::Id) -> cosmic::Element<'_, Self::Message> {
        if self.outputs.iter().any(|o| o.id == id) {
            screenshot::view(self, id).map(Msg::Screenshot)
        } else if let Some(indicator) = &self.recording_indicator {
            if indicator.window_id == id {
                // Render the blinking recording indicator
                render_recording_indicator(indicator)
            } else {
                cosmic::widget::horizontal_space()
                    .width(cosmic::iced_core::Length::Fixed(1.0))
                    .into()
            }
        } else {
            cosmic::widget::horizontal_space()
                .width(cosmic::iced_core::Length::Fixed(1.0))
                .into()
        }
    }

    fn update(
        &mut self,
        message: Self::Message,
    ) -> cosmic::iced::Task<cosmic::Action<Self::Message>> {
        match message {
            Msg::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                key, modifiers, ..
            }) => {
                if let Some(args) = self.screenshot_args.as_ref() {
                    let focused_output_index = args.session.focused_output_index;
                    if let Some(msg) =
                        crate::session::shortcuts::handle_key_event(args, key, modifiers, focused_output_index)
                    {
                        return self.update(Msg::Screenshot(msg));
                    }
                }
                cosmic::iced::Task::none()
            }
            Msg::Keyboard(_) => cosmic::iced::Task::none(),
            Msg::Portal(e) => match e {
                screenshot::Event::Screenshot(args) => {
                    screenshot::update_args(self, args).map(cosmic::Action::App)
                }
                screenshot::Event::Init(tx) => {
                    self.tx = Some(tx);
                    Task::none()
                }
            },
            Msg::Screenshot(m) => screenshot::update_msg(self, m).map(cosmic::Action::App),
            Msg::RecordingBlink => {
                if let Some(indicator) = &mut self.recording_indicator {
                    indicator.blink_visible = !indicator.blink_visible;
                }
                cosmic::iced::Task::none()
            }
            Msg::RecordingStopped => {
                if let Some(indicator) = self.recording_indicator.take() {
                    log::info!("Recording stopped, destroying indicator overlay");
                    return cosmic::iced_winit::commands::layer_surface::destroy_layer_surface(
                        indicator.window_id,
                    );
                }
                cosmic::iced::Task::none()
            }
            Msg::AnnotationFade => {
                if let Some(indicator) = &mut self.recording_indicator {
                    let fade_duration = 3.0; // seconds to fully fade
                    let fade_per_tick = 1.0 / (fade_duration * 20.0); // 20 ticks per second

                    // Update opacity for completed strokes
                    for stroke in &mut indicator.annotations {
                        if stroke.completed_at.is_some() {
                            stroke.opacity = (stroke.opacity - fade_per_tick).max(0.0);
                        }
                    }

                    // Remove fully faded strokes
                    indicator.annotations.retain(|s| s.opacity > 0.0);
                }
                cosmic::iced::Task::none()
            }
            Msg::IndicatorMouse(event, position) => {
                if let Some(indicator) = &mut self.recording_indicator {
                    // Draw when annotation mode is active (pencil button on)
                    let can_draw = indicator.annotation_mode;

                    match event {
                        cosmic::iced::mouse::Event::ButtonPressed(cosmic::iced::mouse::Button::Left) => {
                            if can_draw {
                                // Start a new stroke
                                indicator.current_stroke = Some(vec![(position.x, position.y)]);
                            }
                        }
                        cosmic::iced::mouse::Event::CursorMoved { .. } => {
                            if can_draw {
                                if let Some(stroke) = &mut indicator.current_stroke {
                                    stroke.push((position.x, position.y));
                                }
                            }
                        }
                        cosmic::iced::mouse::Event::ButtonReleased(cosmic::iced::mouse::Button::Left) => {
                            if let Some(points) = indicator.current_stroke.take() {
                                if points.len() > 1 {
                                    indicator.annotations.push(AnnotationStroke {
                                        points,
                                        completed_at: Some(std::time::Instant::now()),
                                        opacity: 1.0,
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
                cosmic::iced::Task::none()
            }
            Msg::IndicatorModifiers(_modifiers) => {
                // Modifier tracking no longer needed for drawing
                // (kept for potential future features)
                cosmic::iced::Task::none()
            }
            Msg::ToggleAnnotationMode => {
                if let Some(indicator) = &mut self.recording_indicator {
                    indicator.annotation_mode = !indicator.annotation_mode;
                    log::info!(
                        "Annotation mode toggled: {}",
                        if indicator.annotation_mode { "ON" } else { "OFF" }
                    );

                    // Recreate the layer surface with appropriate input zone
                    let old_window_id = indicator.window_id;
                    let new_window_id = window::Id::unique();
                    indicator.window_id = new_window_id;

                    let wl_output = indicator.output.clone();
                    let annotation_mode = indicator.annotation_mode;
                    let toolbar_pos = indicator.toolbar_pos;

                    // Toolbar dimensions - must match constants in render_recording_indicator
                    let toolbar_width = 140.0f32;
                    let toolbar_height = 56.0f32;

                    use cosmic::iced_winit::commands::layer_surface::{
                        destroy_layer_surface, get_layer_surface,
                    };
                    use cosmic::iced_runtime::platform_specific::wayland::layer_surface::{
                        IcedOutput, SctkLayerSurfaceSettings,
                    };
                    use cosmic_client_toolkit::sctk::shell::wlr_layer::{
                        Anchor, KeyboardInteractivity, Layer,
                    };
                    use cosmic::iced_core::layout::Limits;

                    let input_zone = if annotation_mode {
                        // Full input capture for drawing
                        None
                    } else {
                        // Only capture input on the toolbar area
                        Some(vec![cosmic::iced_core::Rectangle {
                            x: toolbar_pos.0,
                            y: toolbar_pos.1,
                            width: toolbar_width,
                            height: toolbar_height,
                        }])
                    };

                    let destroy_task = destroy_layer_surface(old_window_id);
                    let create_task = get_layer_surface(SctkLayerSurfaceSettings {
                        id: new_window_id,
                        layer: Layer::Overlay,
                        keyboard_interactivity: if annotation_mode {
                            KeyboardInteractivity::OnDemand
                        } else {
                            KeyboardInteractivity::None
                        },
                        input_zone,
                        anchor: Anchor::all(),
                        output: IcedOutput::Output(wl_output),
                        namespace: "snappea-indicator".to_string(),
                        size: Some((None, None)),
                        exclusive_zone: -1,
                        size_limits: Limits::NONE.min_height(1.0).min_width(1.0),
                        ..Default::default()
                    });

                    return cosmic::Task::batch([destroy_task, create_task]);
                }
                cosmic::iced::Task::none()
            }
            Msg::StopRecording => {
                log::info!("Stop recording requested from toolbar");
                if let Err(e) = crate::screencast::stop_recording() {
                    log::error!("Failed to stop recording: {}", e);
                }
                cosmic::iced::Task::none()
            }
            Msg::ToolbarDragStart(x, y) => {
                if let Some(indicator) = &mut self.recording_indicator {
                    indicator.toolbar_dragging = true;
                    indicator.drag_offset = (
                        x - indicator.toolbar_pos.0,
                        y - indicator.toolbar_pos.1,
                    );
                }
                cosmic::iced::Task::none()
            }
            Msg::ToolbarDragMove(x, y) => {
                if let Some(indicator) = &mut self.recording_indicator {
                    if indicator.toolbar_dragging {
                        indicator.toolbar_pos = (
                            x - indicator.drag_offset.0,
                            y - indicator.drag_offset.1,
                        );
                    }
                }
                cosmic::iced::Task::none()
            }
            Msg::ToolbarDragEnd => {
                if let Some(indicator) = &mut self.recording_indicator {
                    indicator.toolbar_dragging = false;

                    // Only recreate the surface if NOT in annotation mode
                    // (in annotation mode, input_zone is None so position doesn't matter)
                    if !indicator.annotation_mode {
                        // Recreate the layer surface with updated input zone for new toolbar position
                        let old_window_id = indicator.window_id;
                        let new_window_id = window::Id::unique();
                        indicator.window_id = new_window_id;

                        let wl_output = indicator.output.clone();
                        let toolbar_pos = indicator.toolbar_pos;

                        // Toolbar dimensions - must match constants in render_recording_indicator
                        let toolbar_width = 140.0f32;
                        let toolbar_height = 56.0f32;

                        use cosmic::iced_winit::commands::layer_surface::{
                            destroy_layer_surface, get_layer_surface,
                        };
                        use cosmic::iced_runtime::platform_specific::wayland::layer_surface::{
                            IcedOutput, SctkLayerSurfaceSettings,
                        };
                        use cosmic_client_toolkit::sctk::shell::wlr_layer::{
                            Anchor, KeyboardInteractivity, Layer,
                        };
                        use cosmic::iced_core::layout::Limits;

                        let destroy_task = destroy_layer_surface(old_window_id);
                        let create_task = get_layer_surface(SctkLayerSurfaceSettings {
                            id: new_window_id,
                            layer: Layer::Overlay,
                            keyboard_interactivity: KeyboardInteractivity::None,
                            input_zone: Some(vec![cosmic::iced_core::Rectangle {
                                x: toolbar_pos.0,
                                y: toolbar_pos.1,
                                width: toolbar_width,
                                height: toolbar_height,
                            }]),
                            anchor: Anchor::all(),
                            output: IcedOutput::Output(wl_output),
                            namespace: "snappea-indicator".to_string(),
                            size: Some((None, None)),
                            exclusive_zone: -1,
                            size_limits: Limits::NONE.min_height(1.0).min_width(1.0),
                            ..Default::default()
                        });

                        return cosmic::Task::batch([destroy_task, create_task]);
                    }
                }
                cosmic::iced::Task::none()
            }
            Msg::Output(o_event, wl_output) => {
                match o_event {
                    OutputEvent::Created(Some(info))
                        if info.name.is_some()
                            && info.logical_size.is_some()
                            && info.logical_position.is_some() =>
                    {
                        self.outputs.push(OutputState {
                            output: wl_output,
                            id: window::Id::unique(),
                            name: info.name.unwrap(),
                            logical_size: info
                                .logical_size
                                .map(|(w, h)| (w as u32, h as u32))
                                .unwrap(),
                            logical_pos: info.logical_position.unwrap(),
                            scale_factor: info.scale_factor,
                            has_pointer: false,
                            bg_source: None,
                        })
                    }
                    OutputEvent::Removed => self.outputs.retain(|o| o.output != wl_output),
                    OutputEvent::InfoUpdate(info)
                        if info.name.is_some()
                            && info.logical_size.is_some()
                            && info.logical_position.is_some() =>
                    {
                        if let Some(state) = self.outputs.iter_mut().find(|o| o.output == wl_output)
                        {
                            state.name = info.name.unwrap();
                            state.logical_size = info
                                .logical_size
                                .map(|(w, h)| (w as u32, h as u32))
                                .unwrap();
                            state.logical_pos = info.logical_position.unwrap();
                            state.scale_factor = info.scale_factor;
                        } else {
                            log::warn!("Updated output {:?} not found", wl_output);
                            self.outputs.push(OutputState {
                                output: wl_output,
                                id: window::Id::unique(),
                                name: info.name.unwrap(),
                                logical_size: info
                                    .logical_size
                                    .map(|(w, h)| (w as u32, h as u32))
                                    .unwrap(),
                                logical_pos: info.logical_position.unwrap(),
                                scale_factor: info.scale_factor,
                                has_pointer: false,
                                bg_source: None,
                            });
                        }
                    }
                    e => {
                        log::warn!("Unhandled output event: {:?} {e:?}", wl_output);
                    }
                };

                cosmic::iced::Task::none()
            }
        }
    }

    fn subscription(&self) -> cosmic::iced_futures::Subscription<Self::Message> {
        let mut subscriptions = vec![
            portal_subscription(self.wayland_helper.clone()).map(Msg::Portal),
            listen_with(|e, _, _| match e {
                cosmic::iced_core::Event::PlatformSpecific(
                    cosmic::iced_core::event::PlatformSpecific::Wayland(
                        cosmic::iced_core::event::wayland::Event::Output(o_event, wl_output),
                    ),
                ) => Some(Msg::Output(o_event, wl_output)),
                cosmic::iced_core::Event::Keyboard(keyboard_event) => {
                    Some(Msg::Keyboard(keyboard_event))
                }
                _ => None,
            }),
        ];

        // Add timers and event listeners when recording indicator is active
        if self.recording_indicator.is_some() {
            // Blink timer (500ms)
            subscriptions.push(
                cosmic::iced::time::every(std::time::Duration::from_millis(500))
                    .map(|_| Msg::RecordingBlink),
            );

            // Check if recording is still active (every second)
            subscriptions.push(
                cosmic::iced::time::every(std::time::Duration::from_millis(1000)).map(|_| {
                    if crate::screencast::is_recording() {
                        Msg::RecordingBlink // Keep indicator alive (noop blink)
                    } else {
                        Msg::RecordingStopped
                    }
                }),
            );

            // Annotation fade timer (50ms for smooth fading)
            subscriptions.push(
                cosmic::iced::time::every(std::time::Duration::from_millis(50))
                    .map(|_| Msg::AnnotationFade),
            );
        }

        // Add timeline subscription for UI animations when screenshot UI is active
        if let Some(args) = &self.screenshot_args {
            subscriptions.push(
                args.ui.timeline.as_subscription().map(|(window_id, instant)| {
                    Msg::Screenshot(crate::session::messages::Msg::timeline_tick(window_id, instant))
                }),
            );
        }

        Subscription::batch(subscriptions)
    }
}

pub enum SubscriptionState {
    Init,
    Waiting(
        zbus::Connection,
        tokio::sync::mpsc::Receiver<screenshot::Event>,
    ),
}

pub(crate) fn portal_subscription(
    helper: crate::wayland::WaylandHelper,
) -> cosmic::iced::Subscription<screenshot::Event> {
    struct PortalSubscription;
    Subscription::run_with_id(
        TypeId::of::<PortalSubscription>(),
        cosmic::iced_futures::stream::channel(10, |mut output| async move {
            let mut state = SubscriptionState::Init;
            loop {
                if let Err(err) = process_changes(&mut state, &mut output, &helper).await {
                    log::debug!("Portal Subscription Error: {:?}", err);
                    futures::future::pending::<()>().await;
                }
            }
        }),
    )
}

pub(crate) async fn process_changes(
    state: &mut SubscriptionState,
    output: &mut futures::channel::mpsc::Sender<screenshot::Event>,
    wayland_helper: &crate::wayland::WaylandHelper,
) -> anyhow::Result<()> {
    match state {
        SubscriptionState::Init => {
            let (tx, rx) = tokio::sync::mpsc::channel(10);

            let connection = zbus::connection::Builder::session()?
                .name(DBUS_NAME)?
                .serve_at(
                    DBUS_PATH,
                    screenshot::Screenshot::new(wayland_helper.clone(), tx.clone()),
                )?
                .build()
                .await?;
            _ = output.send(screenshot::Event::Init(tx)).await;
            *state = SubscriptionState::Waiting(connection, rx);
        }
        SubscriptionState::Waiting(_conn, rx) => {
            while let Some(event) = rx.recv().await {
                match event {
                    screenshot::Event::Screenshot(args) => {
                        if let Err(err) = output.send(screenshot::Event::Screenshot(args)).await {
                            log::error!("Error sending screenshot event: {:?}", err);
                        };
                    }
                    screenshot::Event::Init(_) => {}
                }
            }
        }
    };
    Ok(())
}

/// Render the recording indicator overlay - a blinking red border and annotations
fn render_recording_indicator(indicator: &RecordingIndicator) -> cosmic::Element<'static, Msg> {
    use cosmic::iced_core::Length;
    use cosmic::iced_widget::canvas::{self, Geometry, Path, Stroke};

    // Clone data for use in the canvas program
    let region = indicator.region;
    let visible = indicator.blink_visible;
    let annotations = indicator.annotations.clone();
    let current_stroke = indicator.current_stroke.clone();
    let super_pressed = indicator.super_pressed;
    let ctrl_pressed = indicator.ctrl_pressed;
    let annotation_mode = indicator.annotation_mode;

    // Toolbar dimensions and position
    let toolbar_pos = indicator.toolbar_pos;
    let toolbar_dragging = indicator.toolbar_dragging;

    // Toolbar layout constants - sized to match main toolbar
    const TOOLBAR_HEIGHT: f32 = 56.0;
    const TOOLBAR_WIDTH: f32 = 140.0;
    const GRAB_HANDLE_WIDTH: f32 = 20.0;
    const BUTTON_SIZE: f32 = 40.0;
    const BUTTON_PADDING: f32 = 8.0;

    struct RecordingOverlay {
        region: (i32, i32, u32, u32),
        border_visible: bool,
        annotations: Vec<AnnotationStroke>,
        current_stroke: Option<Vec<(f32, f32)>>,
        super_pressed: bool,
        ctrl_pressed: bool,
        annotation_mode: bool,
        toolbar_x: f32,
        toolbar_y: f32,
        toolbar_dragging: bool,
    }

    /// State for tracking cursor position between events
    #[derive(Default)]
    struct OverlayState {
        cursor_position: cosmic::iced_core::Point,
    }

    impl canvas::Program<Msg, cosmic::Theme, cosmic::Renderer> for RecordingOverlay {
        type State = OverlayState;

        fn update(
            &self,
            state: &mut Self::State,
            event: canvas::Event,
            bounds: cosmic::iced_core::Rectangle,
            cursor: cosmic::iced_core::mouse::Cursor,
        ) -> (canvas::event::Status, Option<Msg>) {
            use cosmic::iced_core::keyboard;
            use cosmic::iced_core::mouse::{Button, Event as MouseEvent};

            // Update cursor position if available
            if let Some(pos) = cursor.position_in(bounds) {
                state.cursor_position = pos;
            }

            let cx = state.cursor_position.x;
            let cy = state.cursor_position.y;

            // Toolbar hit areas
            let grab_x = self.toolbar_x;
            let grab_y = self.toolbar_y;
            let grab_w = GRAB_HANDLE_WIDTH;
            let grab_h = TOOLBAR_HEIGHT;

            let stop_x = self.toolbar_x + GRAB_HANDLE_WIDTH + BUTTON_PADDING;
            let stop_y = self.toolbar_y + (TOOLBAR_HEIGHT - BUTTON_SIZE) / 2.0;

            let pencil_x = stop_x + BUTTON_SIZE + BUTTON_PADDING;
            let pencil_y = stop_y;

            let is_on_grab = cx >= grab_x && cx <= grab_x + grab_w
                && cy >= grab_y && cy <= grab_y + grab_h;
            let is_on_stop = cx >= stop_x && cx <= stop_x + BUTTON_SIZE
                && cy >= stop_y && cy <= stop_y + BUTTON_SIZE;
            let is_on_pencil = cx >= pencil_x && cx <= pencil_x + BUTTON_SIZE
                && cy >= pencil_y && cy <= pencil_y + BUTTON_SIZE;

            // Check if annotation mode is active (pencil button is on)
            // When pencil mode is on, can draw directly without modifier keys
            let can_draw = self.annotation_mode;

            match event {
                canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                    return (
                        canvas::event::Status::Captured,
                        Some(Msg::IndicatorModifiers(modifiers)),
                    );
                }
                canvas::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
                    // Check toolbar interactions
                    if is_on_grab {
                        return (
                            canvas::event::Status::Captured,
                            Some(Msg::ToolbarDragStart(cx, cy)),
                        );
                    }
                    if is_on_stop {
                        return (
                            canvas::event::Status::Captured,
                            Some(Msg::StopRecording),
                        );
                    }
                    if is_on_pencil {
                        return (
                            canvas::event::Status::Captured,
                            Some(Msg::ToggleAnnotationMode),
                        );
                    }
                    // Start drawing if in annotation mode (pencil button on)
                    if can_draw {
                        return (
                            canvas::event::Status::Captured,
                            Some(Msg::IndicatorMouse(
                                cosmic::iced::mouse::Event::ButtonPressed(
                                    cosmic::iced::mouse::Button::Left,
                                ),
                                state.cursor_position,
                            )),
                        );
                    }
                }
                canvas::Event::Mouse(MouseEvent::CursorMoved { position }) => {
                    state.cursor_position = position;
                    // Handle toolbar dragging
                    if self.toolbar_dragging {
                        return (
                            canvas::event::Status::Captured,
                            Some(Msg::ToolbarDragMove(position.x, position.y)),
                        );
                    }
                    if can_draw && self.current_stroke.is_some() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Msg::IndicatorMouse(
                                cosmic::iced::mouse::Event::CursorMoved { position },
                                position,
                            )),
                        );
                    }
                }
                canvas::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                    // End toolbar drag
                    if self.toolbar_dragging {
                        return (
                            canvas::event::Status::Captured,
                            Some(Msg::ToolbarDragEnd),
                        );
                    }
                    if self.current_stroke.is_some() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Msg::IndicatorMouse(
                                cosmic::iced::mouse::Event::ButtonReleased(
                                    cosmic::iced::mouse::Button::Left,
                                ),
                                state.cursor_position,
                            )),
                        );
                    }
                }
                _ => {}
            }

            // Let events pass through when not drawing
            (canvas::event::Status::Ignored, None)
        }

        fn draw(
            &self,
            _state: &Self::State,
            renderer: &cosmic::Renderer,
            _theme: &cosmic::Theme,
            bounds: cosmic::iced_core::Rectangle,
            _cursor: cosmic::iced_core::mouse::Cursor,
        ) -> Vec<Geometry> {
            let mut frame = canvas::Frame::new(renderer, bounds.size());

            // Draw a red border rectangle OUTSIDE the recording region
            // so it's not captured in the recording
            if self.border_visible {
                let border_width = 4.0;
                // Add safety margin to account for rounding/scaling errors
                let margin = 2.0;
                // Position the path so the border is entirely outside the region
                let x = self.region.0 as f32 - border_width - margin;
                let y = self.region.1 as f32 - border_width - margin;
                let w = self.region.2 as f32 + (border_width + margin) * 2.0;
                let h = self.region.3 as f32 + (border_width + margin) * 2.0;

                let path = Path::rectangle(
                    cosmic::iced_core::Point::new(x, y),
                    cosmic::iced_core::Size::new(w, h),
                );

                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(cosmic::iced_core::Color::from_rgb(1.0, 0.0, 0.0))
                        .with_width(border_width),
                );
            }

            // Draw completed annotation strokes with fading opacity
            for stroke in &self.annotations {
                if stroke.points.len() < 2 {
                    continue;
                }

                let path = Path::new(|builder| {
                    builder.move_to(cosmic::iced_core::Point::new(
                        stroke.points[0].0,
                        stroke.points[0].1,
                    ));
                    for point in &stroke.points[1..] {
                        builder.line_to(cosmic::iced_core::Point::new(point.0, point.1));
                    }
                });

                // Yellow color with fading opacity
                let color = cosmic::iced_core::Color::from_rgba(1.0, 0.9, 0.0, stroke.opacity);

                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(color)
                        .with_width(3.0)
                        .with_line_cap(canvas::LineCap::Round)
                        .with_line_join(canvas::LineJoin::Round),
                );
            }

            // Draw current stroke being drawn (full opacity)
            if let Some(points) = &self.current_stroke {
                if points.len() >= 2 {
                    let path = Path::new(|builder| {
                        builder.move_to(cosmic::iced_core::Point::new(points[0].0, points[0].1));
                        for point in &points[1..] {
                            builder.line_to(cosmic::iced_core::Point::new(point.0, point.1));
                        }
                    });

                    // Yellow color, full opacity for current stroke
                    let color = cosmic::iced_core::Color::from_rgba(1.0, 0.9, 0.0, 1.0);

                    frame.stroke(
                        &path,
                        Stroke::default()
                            .with_color(color)
                            .with_width(3.0)
                            .with_line_cap(canvas::LineCap::Round)
                            .with_line_join(canvas::LineJoin::Round),
                    );
                }
            }

            // Draw toolbar with grab handle, stop button, and pencil button
            // Style matches the main screenshot toolbar (rounded corners, semi-transparent background)
            {
                let tx = self.toolbar_x;
                let ty = self.toolbar_y;
                let radius = 8.0; // Matches cosmic radius_s

                // Toolbar background (rounded rectangle with subtle border)
                let toolbar_bg = Path::new(|builder| {
                    builder.move_to(cosmic::iced_core::Point::new(tx + radius, ty));
                    builder.line_to(cosmic::iced_core::Point::new(tx + TOOLBAR_WIDTH - radius, ty));
                    builder.arc_to(
                        cosmic::iced_core::Point::new(tx + TOOLBAR_WIDTH, ty),
                        cosmic::iced_core::Point::new(tx + TOOLBAR_WIDTH, ty + radius),
                        radius,
                    );
                    builder.line_to(cosmic::iced_core::Point::new(tx + TOOLBAR_WIDTH, ty + TOOLBAR_HEIGHT - radius));
                    builder.arc_to(
                        cosmic::iced_core::Point::new(tx + TOOLBAR_WIDTH, ty + TOOLBAR_HEIGHT),
                        cosmic::iced_core::Point::new(tx + TOOLBAR_WIDTH - radius, ty + TOOLBAR_HEIGHT),
                        radius,
                    );
                    builder.line_to(cosmic::iced_core::Point::new(tx + radius, ty + TOOLBAR_HEIGHT));
                    builder.arc_to(
                        cosmic::iced_core::Point::new(tx, ty + TOOLBAR_HEIGHT),
                        cosmic::iced_core::Point::new(tx, ty + TOOLBAR_HEIGHT - radius),
                        radius,
                    );
                    builder.line_to(cosmic::iced_core::Point::new(tx, ty + radius));
                    builder.arc_to(
                        cosmic::iced_core::Point::new(tx, ty),
                        cosmic::iced_core::Point::new(tx + radius, ty),
                        radius,
                    );
                    builder.close();
                });

                // Dark semi-transparent background (matches cosmic theme component background)
                frame.fill(&toolbar_bg, cosmic::iced_core::Color::from_rgba(0.12, 0.12, 0.14, 0.92));

                // Subtle border
                frame.stroke(
                    &toolbar_bg,
                    Stroke::default()
                        .with_color(cosmic::iced_core::Color::from_rgba(0.3, 0.3, 0.35, 0.5))
                        .with_width(1.0),
                );

                // Grab handle (vertical dots/lines on the left)
                let grab_center_x = tx + GRAB_HANDLE_WIDTH / 2.0;
                let grab_center_y = ty + TOOLBAR_HEIGHT / 2.0;
                for dy in [-12.0, 0.0, 12.0] {
                    let dot = Path::circle(
                        cosmic::iced_core::Point::new(grab_center_x - 3.0, grab_center_y + dy),
                        2.5,
                    );
                    frame.fill(&dot, cosmic::iced_core::Color::from_rgba(0.55, 0.55, 0.55, 1.0));
                    let dot2 = Path::circle(
                        cosmic::iced_core::Point::new(grab_center_x + 3.0, grab_center_y + dy),
                        2.5,
                    );
                    frame.fill(&dot2, cosmic::iced_core::Color::from_rgba(0.55, 0.55, 0.55, 1.0));
                }

                // Stop button (red square/circle)
                let stop_x = tx + GRAB_HANDLE_WIDTH + BUTTON_PADDING;
                let stop_y = ty + (TOOLBAR_HEIGHT - BUTTON_SIZE) / 2.0;
                let stop_center_x = stop_x + BUTTON_SIZE / 2.0;
                let stop_center_y = stop_y + BUTTON_SIZE / 2.0;

                let stop_bg = Path::circle(
                    cosmic::iced_core::Point::new(stop_center_x, stop_center_y),
                    BUTTON_SIZE / 2.0,
                );
                frame.fill(&stop_bg, cosmic::iced_core::Color::from_rgba(0.8, 0.2, 0.2, 1.0));

                // Stop icon (white square)
                let stop_icon_size = BUTTON_SIZE * 0.35;
                let stop_icon = Path::rectangle(
                    cosmic::iced_core::Point::new(
                        stop_center_x - stop_icon_size / 2.0,
                        stop_center_y - stop_icon_size / 2.0,
                    ),
                    cosmic::iced_core::Size::new(stop_icon_size, stop_icon_size),
                );
                frame.fill(&stop_icon, cosmic::iced_core::Color::WHITE);

                // Pencil button
                let pencil_x = stop_x + BUTTON_SIZE + BUTTON_PADDING;
                let pencil_y = stop_y;
                let pencil_center_x = pencil_x + BUTTON_SIZE / 2.0;
                let pencil_center_y = pencil_y + BUTTON_SIZE / 2.0;

                // Pencil button background - green when active, gray otherwise
                let pencil_bg_color = if self.annotation_mode {
                    cosmic::iced_core::Color::from_rgba(0.2, 0.7, 0.2, 1.0)
                } else {
                    cosmic::iced_core::Color::from_rgba(0.4, 0.4, 0.4, 1.0)
                };

                let pencil_bg = Path::circle(
                    cosmic::iced_core::Point::new(pencil_center_x, pencil_center_y),
                    BUTTON_SIZE / 2.0,
                );
                frame.fill(&pencil_bg, pencil_bg_color);

                // Pencil icon (diagonal line with tip)
                let icon_size = BUTTON_SIZE * 0.4;
                let pen_path = Path::new(|builder| {
                    builder.move_to(cosmic::iced_core::Point::new(
                        pencil_center_x - icon_size * 0.4,
                        pencil_center_y + icon_size * 0.4,
                    ));
                    builder.line_to(cosmic::iced_core::Point::new(
                        pencil_center_x + icon_size * 0.4,
                        pencil_center_y - icon_size * 0.4,
                    ));
                });

                frame.stroke(
                    &pen_path,
                    Stroke::default()
                        .with_color(cosmic::iced_core::Color::WHITE)
                        .with_width(3.5)
                        .with_line_cap(canvas::LineCap::Round),
                );

                // Pencil tip
                let tip_path = Path::circle(
                    cosmic::iced_core::Point::new(
                        pencil_center_x - icon_size * 0.45,
                        pencil_center_y + icon_size * 0.45,
                    ),
                    3.0,
                );
                frame.fill(&tip_path, cosmic::iced_core::Color::WHITE);
            }

            vec![frame.into_geometry()]
        }
    }

    let program = RecordingOverlay {
        region,
        border_visible: visible,
        annotations,
        current_stroke,
        super_pressed,
        ctrl_pressed,
        annotation_mode,
        toolbar_x: toolbar_pos.0,
        toolbar_y: toolbar_pos.1,
        toolbar_dragging,
    };

    canvas::Canvas::new(program)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

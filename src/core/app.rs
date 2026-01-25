use crate::core::portal::{DBUS_NAME, DBUS_PATH};
use crate::screenshot;
use crate::session::messages;
use crate::session::state::SettingsTab;
use crate::tray::{self, TrayAction, TrayHandle};
use cosmic::Task;
use cosmic::iced_core::event::wayland::OutputEvent;
use cosmic::widget::segmented_button;
use cosmic::{
    app,
    iced::window,
    iced_futures::{Subscription, event::listen_with},
};
use crossbeam_channel::{Receiver as CbReceiver, Sender as CbSender};
use futures::SinkExt;
use std::any::TypeId;
use wayland_client::protocol::wl_output::WlOutput;

pub(crate) fn run() -> cosmic::iced::Result {
    let settings = cosmic::app::Settings::default()
        .no_main_window(true)
        .exit_on_close(false);
    cosmic::app::run::<App>(settings, ())
}

/// Create a new settings tab segmented button model
pub fn create_settings_tab_model() -> segmented_button::SingleSelectModel {
    segmented_button::Model::builder()
        .insert(|b| b.text("General").data(SettingsTab::General).activate())
        .insert(|b| b.text("Picture").data(SettingsTab::Picture))
        .insert(|b| b.text("Video").data(SettingsTab::Video))
        .build()
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
    /// Settings tab segmented button model (stored here since it's not Send)
    pub settings_tab_model: segmented_button::SingleSelectModel,
    /// Tray icon handle (only active during recording if hide_toolbar_to_tray is enabled)
    pub tray_handle: Option<TrayHandle>,
    /// Tray action receiver
    pub tray_rx: Option<CbReceiver<TrayAction>>,
    /// Tray action sender (kept for creating new tray instances)
    pub tray_tx: Option<CbSender<TrayAction>>,
    /// Whether the toolbar is currently visible (when using tray mode)
    pub toolbar_visible: bool,
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
    /// Color of this stroke (RGB, 0.0-1.0)
    pub color: crate::config::ShapeColor,
    /// Line thickness in pixels
    pub thickness: f32,
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
    /// Output size in logical pixels (for popup positioning)
    pub output_size: (f32, f32),
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
    /// Pencil color (RGB, 0.0-1.0)
    pub pencil_color: crate::config::ShapeColor,
    /// Duration in seconds before pencil strokes fade away
    pub pencil_fade_duration: f32,
    /// Pencil line thickness in pixels
    pub pencil_thickness: f32,
    /// Toolbar bounds from main UI (output-local coords)
    pub toolbar_bounds: Option<cosmic::iced_core::Rectangle>,
    /// Toolbar position (top-left corner)
    pub toolbar_pos: (f32, f32),
    /// Whether toolbar is being dragged
    pub toolbar_dragging: bool,
    /// Drag offset from toolbar top-left when drag started
    pub drag_offset: (f32, f32),
    /// Whether pencil popup is open
    pub pencil_popup_open: bool,
    /// Pencil popup bounds for input zone calculation
    pub pencil_popup_bounds: Option<cosmic::iced_core::Rectangle>,
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
    /// Tray action received
    TrayAction(TrayAction),
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
        // Create channel for tray communication
        let (tray_tx, tray_rx) = crossbeam_channel::unbounded::<TrayAction>();
        
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
                settings_tab_model: create_settings_tab_model(),
                tray_handle: None,
                tray_rx: Some(tray_rx),
                tray_tx: Some(tray_tx),
                toolbar_visible: true,
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
                render_recording_indicator(indicator, self.toolbar_visible)
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
                // Clean up tray if active
                if let Some(handle) = self.tray_handle.take() {
                    handle.shutdown();
                }
                self.toolbar_visible = true;
                
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
                    let total_duration = indicator.pencil_fade_duration;
                    let hold_duration = total_duration * 0.8; // Stay opaque for 80%
                    let fade_duration = total_duration * 0.2; // Fade during last 20%

                    // Update opacity for completed strokes
                    for stroke in &mut indicator.annotations {
                        if let Some(completed_at) = stroke.completed_at {
                            let elapsed = completed_at.elapsed().as_secs_f32();

                            if elapsed <= hold_duration {
                                // During hold phase, stay fully opaque
                                stroke.opacity = 1.0;
                            } else {
                                // During fade phase, linearly interpolate from 1.0 to 0.0
                                let fade_elapsed = elapsed - hold_duration;
                                let fade_progress = (fade_elapsed / fade_duration).min(1.0);
                                stroke.opacity = 1.0 - fade_progress;
                            }
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
                                    // Capture current color and thickness for this stroke
                                    let color = indicator.pencil_color;
                                    let thickness = indicator.pencil_thickness;
                                    indicator.annotations.push(AnnotationStroke {
                                        points,
                                        completed_at: Some(std::time::Instant::now()),
                                        opacity: 1.0,
                                        color,
                                        thickness,
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
                // Note: annotation_mode is already set by the caller (ToggleRecordingAnnotation or PencilPopup handler)
                // This message just recreates the layer surface with the updated input zone
                if let Some(indicator) = &mut self.recording_indicator {
                    log::info!(
                        "Recreating annotation surface, mode: {}",
                        if indicator.annotation_mode { "ON" } else { "OFF" }
                    );

                    // Recreate the layer surface with appropriate input zone
                    let old_window_id = indicator.window_id;
                    let new_window_id = window::Id::unique();
                    indicator.window_id = new_window_id;

                    let wl_output = indicator.output.clone();
                    let annotation_mode = indicator.annotation_mode;

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
                        // Annotation mode: capture region for drawing + toolbar for controls
                        let region_rect = cosmic::iced_core::Rectangle {
                            x: indicator.region.0 as f32,
                            y: indicator.region.1 as f32,
                            width: indicator.region.2 as f32,
                            height: indicator.region.3 as f32,
                        };

                        let mut zones = vec![region_rect];

                        // ALWAYS add toolbar, even if it overlaps region
                        // Widget stacking order (toolbar on top of canvas) ensures toolbar gets priority
                        if let Some(toolbar_bounds) = indicator.toolbar_bounds {
                            zones.push(toolbar_bounds);
                        }

                        // Add popup with HIGHEST priority (added last = checked first by Wayland)
                        if let Some(popup_bounds) = indicator.pencil_popup_bounds {
                            zones.push(popup_bounds);
                        }

                        Some(zones)
                    } else {
                        // No annotation mode: capture toolbar + popup for controls
                        // Desktop is fully interactive everywhere else
                        let mut zones = Vec::new();

                        if let Some(toolbar_bounds) = indicator.toolbar_bounds {
                            zones.push(toolbar_bounds);
                        }

                        // Add popup with HIGHEST priority
                        if let Some(popup_bounds) = indicator.pencil_popup_bounds {
                            zones.push(popup_bounds);
                        }

                        if zones.is_empty() {
                            Some(vec![])
                        } else {
                            Some(zones)
                        }
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
            Msg::TrayAction(action) => {
                match action {
                    TrayAction::StopRecording => {
                        log::info!("Tray: Stop recording requested");
                        // Clean up tray
                        if let Some(handle) = self.tray_handle.take() {
                            handle.shutdown();
                        }
                        // Stop recording via screenshot module
                        return self.update(Msg::Screenshot(
                            crate::session::messages::Msg::Capture(
                                crate::session::messages::CaptureMsg::StopRecording
                            )
                        ));
                    }
                    TrayAction::ToggleToolbar => {
                        log::info!("Tray: Toggle toolbar requested");
                        self.toolbar_visible = !self.toolbar_visible;
                        
                        // Update tray state
                        if let Some(ref handle) = self.tray_handle {
                            let visible = self.toolbar_visible;
                            handle.update(move |tray| {
                                tray.set_toolbar_visible(visible);
                            });
                        }
                        
                        // Recreate the indicator surface with/without toolbar input zone
                        if self.recording_indicator.is_some() {
                            return cosmic::Task::done(cosmic::Action::App(Msg::ToggleAnnotationMode));
                        }
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
        
        // Add tray subscription if we have a receiver
        if let Some(ref rx) = self.tray_rx {
            subscriptions.push(tray_subscription(rx.clone()));
        }

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

/// Subscription for receiving tray actions
fn tray_subscription(rx: CbReceiver<TrayAction>) -> Subscription<Msg> {
    struct TraySub;

    Subscription::run_with_id(
        TypeId::of::<TraySub>(),
        cosmic::iced::stream::channel(10, move |mut output| async move {
            use cosmic::iced_futures::futures::StreamExt;
            
            // Bridge the blocking crossbeam receiver into an async stream
            let (mut tx, mut async_rx) =
                cosmic::iced_futures::futures::channel::mpsc::channel::<TrayAction>(10);

            std::thread::spawn(move || {
                for action in rx.iter() {
                    let _ = tx.try_send(action);
                }
            });

            while let Some(action) = async_rx.next().await {
                if output.send(Msg::TrayAction(action)).await.is_err() {
                    break;
                }
            }
        }),
    )
}

/// Render the recording indicator overlay - a blinking red border and annotations
fn render_recording_indicator(indicator: &RecordingIndicator, toolbar_visible: bool) -> cosmic::Element<'static, Msg> {
    use cosmic::iced_core::Length;
    use cosmic::iced_widget::canvas::{self, Geometry, Path, Stroke};

    // Clone data for use in the canvas program
    let region = indicator.region;
    let visible = indicator.blink_visible;
    let annotations = indicator.annotations.clone();
    let current_stroke = indicator.current_stroke.clone();
    let annotation_mode = indicator.annotation_mode;
    let pencil_color = indicator.pencil_color;
    let pencil_thickness = indicator.pencil_thickness;
    let pencil_popup_open = indicator.pencil_popup_open;
    let pencil_fade_duration = indicator.pencil_fade_duration;

    struct RecordingOverlay {
        region: (i32, i32, u32, u32),
        border_visible: bool,
        annotations: Vec<AnnotationStroke>,
        current_stroke: Option<Vec<(f32, f32)>>,
        annotation_mode: bool,
        pencil_color: crate::config::ShapeColor,
        pencil_thickness: f32,
        pencil_popup_open: bool,
        pencil_popup_bounds: Option<cosmic::iced_core::Rectangle>,
        toolbar_bounds: Option<cosmic::iced_core::Rectangle>,
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
            use cosmic::iced_core::mouse::{Button, Event as MouseEvent};

            // Update cursor position if available
            if let Some(pos) = cursor.position_in(bounds) {
                state.cursor_position = pos;
            }

            match event {
                canvas::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
                    // Handle click-outside-to-close for popup
                    if self.pencil_popup_open {
                        if let Some(cursor_pos) = cursor.position() {
                            let in_popup = self.pencil_popup_bounds
                                .map(|b| b.contains(cursor_pos))
                                .unwrap_or(false);
                            let in_toolbar = self.toolbar_bounds
                                .map(|b| b.contains(cursor_pos))
                                .unwrap_or(false);

                            // Close popup if clicking outside popup and toolbar
                            if !in_popup && !in_toolbar {
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Msg::Screenshot(crate::session::messages::Msg::Tool(
                                        crate::session::messages::ToolMsg::PencilPopup(
                                            crate::session::messages::ToolPopupAction::Close
                                        )
                                    ))),
                                );
                            }
                        }
                    }

                    // Start drawing if in annotation mode (and popup not handling the click)
                    if self.annotation_mode && !self.pencil_popup_open {
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
                    // Don't capture movements if popup is open (even mid-stroke)
                    if self.annotation_mode && self.current_stroke.is_some() && !self.pencil_popup_open {
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
                    // Don't capture release if popup is open (even mid-stroke)
                    if self.current_stroke.is_some() && !self.pencil_popup_open {
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
            // Each stroke has its own color and thickness
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

                // Use per-stroke color with fading opacity
                let color = cosmic::iced_core::Color::from_rgba(
                    stroke.color.r,
                    stroke.color.g,
                    stroke.color.b,
                    stroke.opacity,
                );

                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(color)
                        .with_width(stroke.thickness)
                        .with_line_cap(canvas::LineCap::Round)
                        .with_line_join(canvas::LineJoin::Round),
                );
            }

            // Draw current stroke being drawn (full opacity, uses current settings)
            if let Some(points) = &self.current_stroke {
                if points.len() >= 2 {
                    let path = Path::new(|builder| {
                        builder.move_to(cosmic::iced_core::Point::new(points[0].0, points[0].1));
                        for point in &points[1..] {
                            builder.line_to(cosmic::iced_core::Point::new(point.0, point.1));
                        }
                    });

                    // Use current pencil color, full opacity for stroke being drawn
                    let color = cosmic::iced_core::Color::from_rgba(
                        self.pencil_color.r,
                        self.pencil_color.g,
                        self.pencil_color.b,
                        1.0,
                    );

                    frame.stroke(
                        &path,
                        Stroke::default()
                            .with_color(color)
                            .with_width(self.pencil_thickness)
                            .with_line_cap(canvas::LineCap::Round)
                            .with_line_join(canvas::LineJoin::Round),
                    );
                }
            }

            vec![frame.into_geometry()]
        }
    }

    let program = RecordingOverlay {
        region,
        border_visible: visible,
        annotations,
        current_stroke,
        annotation_mode,
        pencil_color,
        pencil_thickness,
        pencil_popup_open,
        pencil_popup_bounds: indicator.pencil_popup_bounds,
        toolbar_bounds: indicator.toolbar_bounds,
    };

    let canvas_layer = canvas::Canvas::new(program)
        .width(Length::Fill)
        .height(Length::Fill);

    // Add toolbar with stop and pencil toggle buttons
    use cosmic::widget::{button, container, icon};
    use cosmic::iced_widget::row;
    use cosmic::iced_core::Background;

    let toolbar_pos = indicator.toolbar_pos;

    // Stop button - red circle with stop icon
    let stop_icon = container(
        icon::Icon::from(icon::from_name("media-playback-stop-symbolic").size(20))
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0)),
    )
    .class(cosmic::theme::Container::Custom(Box::new(|_theme| {
        cosmic::iced::widget::container::Style {
            background: Some(Background::Color(cosmic::iced_core::Color::from_rgb(0.85, 0.2, 0.2))),
            border: cosmic::iced_core::Border {
                radius: 20.0.into(),
                width: 2.0,
                color: cosmic::iced_core::Color::WHITE,
            },
            ..Default::default()
        }
    })))
    .padding(10)
    .width(Length::Fixed(40.0))
    .height(Length::Fixed(40.0))
    .align_x(cosmic::iced_core::alignment::Horizontal::Center)
    .align_y(cosmic::iced_core::alignment::Vertical::Center);

    let btn_stop = cosmic::widget::tooltip(
        button::custom(stop_icon)
            .class(cosmic::theme::Button::Icon)
            .on_press(Msg::Screenshot(crate::session::messages::Msg::Capture(
                crate::session::messages::CaptureMsg::StopRecording,
            )))
            .padding(0),
        "Stop Recording",
        cosmic::widget::tooltip::Position::Bottom,
    );

    // Drag handle on the left
    const DRAG_ICON: &[u8] = include_bytes!("../../data/icons/hicolor/scalable/actions/drag.svg");
    let drag_handle_icon = cosmic::widget::icon(cosmic::widget::icon::from_svg_bytes(DRAG_ICON).symbolic(true))
        .size(40);
    let drag_handle = cosmic::widget::container(drag_handle_icon)
        .width(Length::Fixed(56.0))
        .height(Length::Fixed(56.0))
        .align_x(cosmic::iced_core::alignment::Horizontal::Center)
        .align_y(cosmic::iced_core::alignment::Vertical::Center);

    // Pencil toggle button with indicator dot and popup support
    let btn_pencil: cosmic::Element<'static, Msg> = crate::widget::tool_button::build_tool_button(
        "edit-symbolic",
        "Freehand Annotation (right-click for options)",
        1, // Single indicator dot (on/off state)
        0, // Always show first dot as active when mode is on
        annotation_mode,
        pencil_popup_open,
        true, // Always enabled during recording
        Some(Msg::Screenshot(crate::session::messages::Msg::Capture(
            crate::session::messages::CaptureMsg::ToggleRecordingAnnotation,
        ))),
        Some(Msg::Screenshot(crate::session::messages::Msg::Capture(
            crate::session::messages::CaptureMsg::PencilRightClick,
        ))),
        8, // padding
        1.0, // full opacity
    );

    // Hide to tray button (minimize icon)
    const MINIMIZE_ICON: &[u8] = include_bytes!("../../data/icons/hicolor/scalable/actions/minimize.svg");
    let tray_icon = cosmic::widget::icon(cosmic::widget::icon::from_svg_bytes(MINIMIZE_ICON).symbolic(true))
        .size(40);

    let btn_hide_to_tray = cosmic::widget::tooltip(
        button::custom(tray_icon)
            .class(cosmic::theme::Button::Icon)
            .on_press(Msg::Screenshot(crate::session::messages::Msg::Capture(
                crate::session::messages::CaptureMsg::HideToTray,
            )))
            .padding(8),
        "Minimize to System Tray",
        cosmic::widget::tooltip::Position::Bottom,
    );

    // Build toolbar content: drag handle | pencil | stop | minimize
    let toolbar_content = row![drag_handle, btn_pencil, btn_stop, btn_hide_to_tray]
        .spacing(8)
        .align_y(cosmic::iced_core::Alignment::Center);

    // Build pencil popup if open
    let pencil_popup = if pencil_popup_open {
        Some(crate::widget::tool_button::build_pencil_popup(
            pencil_color,
            pencil_fade_duration,
            pencil_thickness,
            true, // has annotations (always enable clear during recording)
            &|c| Msg::Screenshot(crate::session::messages::Msg::Tool(
                crate::session::messages::ToolMsg::SetPencilColor(c)
            )),
            |d| Msg::Screenshot(crate::session::messages::Msg::Tool(
                crate::session::messages::ToolMsg::SetPencilFadeDuration(d)
            )),
            Msg::Screenshot(crate::session::messages::Msg::Tool(
                crate::session::messages::ToolMsg::SavePencilFadeDuration
            )),
            |t| Msg::Screenshot(crate::session::messages::Msg::Tool(
                crate::session::messages::ToolMsg::SetPencilThickness(t)
            )),
            Msg::Screenshot(crate::session::messages::Msg::Tool(
                crate::session::messages::ToolMsg::SavePencilThickness
            )),
            Msg::Screenshot(crate::session::messages::Msg::Tool(
                crate::session::messages::ToolMsg::ClearPencilDrawings
            )),
            16, // space_s
            8,  // space_xs
        ))
    } else {
        None
    };

    // Stack canvas, toolbar, and popup with proper vertical positioning
    use cosmic::iced_widget::{column, stack};

    // Toolbar styled container
    let toolbar_with_bg = cosmic::widget::container(toolbar_content)
        .padding(8)
        .class(cosmic::theme::Container::Custom(Box::new(|theme| {
            let cosmic_theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(cosmic_theme.background.component.base.into())),
                border: cosmic::iced_core::Border {
                    radius: cosmic_theme.corner_radii.radius_s.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })));

    // Toolbar layer - pushed to bottom with vertical_space, centered horizontally
    let toolbar_layer = column![
        cosmic::widget::vertical_space(),
        cosmic::widget::container(toolbar_with_bg)
            .center_x(Length::Fill)
    ]
    .padding([0, 0, 32, 0]) // 32px bottom margin
    .width(Length::Fill)
    .height(Length::Fill);

    // Build stack with popup above toolbar if open
    // Build the final stack based on toolbar visibility
    if toolbar_visible {
        if let Some(popup) = pencil_popup {
            // Popup layer - positioned ABOVE toolbar
            let popup_layer = column![
                cosmic::widget::vertical_space(),
                cosmic::widget::container(popup)
                    .center_x(Length::Fill)
            ]
            .padding([0, 0, 140, 0]) // 140px from bottom (32 + 56 toolbar + 52 gap)
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
    } else {
        // No toolbar - just show canvas with red border and annotations
        stack![canvas_layer]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

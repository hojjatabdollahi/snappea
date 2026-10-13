// SPDX-License-Identifier: GPL-3.0-only

use crate::capture::countdown::Msg as CountdownMsg;
use crate::capture::msg;
use crate::dbus::{CONTROL_PATH, ControlCommand, ControlInterface};
use crate::dbus::{DBUS_NAME, DBUS_PATH};
use crate::fl;
use crate::recording::overlay::{
    CHROME_NAMESPACE, Msg as RecordingMsg, RecordingIndicator, render_recording_annotations,
    render_recording_chrome,
};
use cosmic::Task;
use cosmic::cctk::sctk::shell::wlr_layer;
use cosmic::iced::core::event::wayland::OutputEvent;
use cosmic::iced::core::layout::Limits;
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{
    IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced::platform_specific::shell::commands::layer_surface::get_layer_surface;
use cosmic::{
    app,
    iced::window,
    iced::{Subscription, event::listen_with},
};
use futures::SinkExt;
use std::any::TypeId;
use std::time::Instant;
use wayland_client::protocol::wl_output::WlOutput;

/// Flags for app initialization
#[derive(Clone, Debug, Default)]
pub struct AppFlags {
    /// If true, take a screenshot directly without D-Bus portal
    pub direct_screenshot: bool,
}

pub fn run() -> cosmic::iced::Result {
    run_with_flags(AppFlags::default())
}

pub fn run_with_flags(flags: AppFlags) -> cosmic::iced::Result {
    let settings = cosmic::app::Settings::default()
        .no_main_window(true)
        .exit_on_close(false);
    cosmic::app::run::<App>(settings, flags)
}

pub struct App {
    pub core: app::Core,
    pub tx: Option<tokio::sync::mpsc::Sender<crate::dbus::Event>>,
    pub capture: Option<crate::capture::Capture>,
    pub wayland_helper: crate::wayland::WaylandHelper,
    pub outputs: Vec<OutputState>,
    pub qr_scan: Option<cosmic::iced::task::Handle>,
    pub color_picker: cosmic::widget::ColorPickerModel,
    pub text: Option<crate::capture::text::TextEdit>,
    pub font_families: Vec<&'static str>,
    pub text_style_model: cosmic::widget::segmented_button::MultiSelectModel,
    pub text_align_model: cosmic::widget::segmented_button::SingleSelectModel,
    pub countdown: Option<crate::capture::countdown::Countdown>,
    pub recording_indicator: Option<RecordingIndicator>,
    /// Whether running in direct screenshot mode (no D-Bus portal)
    pub direct_screenshot: bool,
    /// True when screenshot capture arrived before any Wayland outputs were known.
    /// The `OutputEvent::Created` handler will create the layer surface on first output.
    pub screenshot_windows_pending: bool,
    /// Dummy layer surface held so the app retains a Wayland surface for clipboard ownership
    pub dummy_id: window::Id,
}

/// Sleep out the whole delay in one go, so the shown seconds and the shutter share a deadline.
pub fn countdown_fire(countdown: &crate::capture::countdown::Countdown) -> cosmic::iced::Task<Msg> {
    let started = countdown.started;
    let remaining = countdown
        .duration
        .saturating_sub(Instant::now().saturating_duration_since(started));
    cosmic::iced::Task::perform(tokio::time::sleep(remaining), move |()| {
        Msg::Countdown(CountdownMsg::Fire(started))
    })
}

/// Wake when the delay button's numeral next changes, scheduled against the deadline.
pub fn countdown_refresh(
    countdown: &crate::capture::countdown::Countdown,
) -> cosmic::iced::Task<Msg> {
    let started = countdown.started;
    let now = Instant::now();
    let left = countdown
        .duration
        .saturating_sub(now.saturating_duration_since(started));
    let step = left.as_nanos() % 1_000_000_000;
    let until = if step == 0 {
        std::time::Duration::from_secs(1)
    } else {
        std::time::Duration::from_nanos(step as u64)
    } + std::time::Duration::from_millis(10);

    cosmic::iced::Task::perform(tokio::time::sleep(until), move |()| {
        Msg::Countdown(CountdownMsg::Refresh(started))
    })
}

#[derive(Debug, Clone)]
pub struct OutputState {
    pub output: WlOutput,
    pub id: window::Id,
    pub name: String,
    pub logical_size: (u32, u32),
    pub logical_pos: (i32, i32),
    pub scale_factor: i32,
}

impl OutputState {
    /// The screen's area in global logical coordinates.
    pub const fn rect(&self) -> crate::geometry::Rect {
        crate::geometry::Rect {
            left: self.logical_pos.0,
            top: self.logical_pos.1,
            right: self.logical_pos.0 + self.logical_size.0.cast_signed(),
            bottom: self.logical_pos.1 + self.logical_size.1.cast_signed(),
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Msg {
    Screenshot(msg::Msg),
    Portal(crate::dbus::Event),
    Output(OutputEvent, WlOutput),
    Keyboard(cosmic::iced::keyboard::Event),
    /// Discard a task's result without acting on it
    Ignore,
    /// Recording has stopped
    RecordingStopped,
    /// The recording overlay: chrome, live annotations, toolbar drag.
    Recording(RecordingMsg),
    /// D-Bus control command received
    Control(ControlCommand),
    /// A layer surface was closed by the compositor (e.g. output disconnected)
    LayerClosed(window::Id),
    /// Failsafe fired ~500 ms after screenshot capture arrive if no windows were created.
    RetryPendingWindows,
    /// The delayed capture's countdown: its timers, the toolbar morph and the pill.
    Countdown(CountdownMsg),
    /// A delayed screenshot's wait elapsed and fresh screen pixels were captured.
    /// Swap them into the existing session and reopen the overlay.
    DelayedCaptureReady(std::collections::HashMap<String, crate::capture::ScreenshotImage>),
}

/// The overlay surface each output carries for a session: selection UI, countdown,
/// and the recording chrome on the recorded output. Capture-excluded, since
/// nothing it draws belongs in a capture.
pub fn overlay_surface<M>(output: &OutputState) -> cosmic::iced::Task<M> {
    get_layer_surface(SctkLayerSurfaceSettings {
        id: output.id,
        layer: wlr_layer::Layer::Overlay,
        keyboard_interactivity: wlr_layer::KeyboardInteractivity::Exclusive,
        input_zone: None,
        anchor: wlr_layer::Anchor::all(),
        output: IcedOutput::Output(output.output.clone()),
        namespace: CHROME_NAMESPACE.to_string(),
        size: Some((None, None)),
        exclusive_zone: -1,
        size_limits: Limits::NONE.min_height(1.0).min_width(1.0),
        ..Default::default()
    })
}

/// A tiny surface on the bottom layer, held so the app always owns a Wayland
/// surface for clipboard ownership.
fn dummy_surface<M>(id: window::Id) -> cosmic::iced::Task<M> {
    get_layer_surface(SctkLayerSurfaceSettings {
        id,
        layer: wlr_layer::Layer::Bottom,
        keyboard_interactivity: wlr_layer::KeyboardInteractivity::OnDemand,
        input_zone: Some(Vec::new()),
        anchor: wlr_layer::Anchor::empty(),
        output: IcedOutput::All,
        namespace: "snappea_dummy".into(),
        size: Some((Some(6), Some(6))),
        exclusive_zone: -1,
        size_limits: Limits::NONE,
        ..Default::default()
    })
}

impl App {
    /// Map the session's overlay, one surface per output.
    pub fn open_overlay<M: 'static>(&mut self) -> cosmic::iced::Task<M> {
        for output in &mut self.outputs {
            output.id = window::Id::unique();
        }
        cosmic::Task::batch(self.outputs.iter().map(overlay_surface))
    }

    /// Hand the overlay to the countdown: input shrinks to the toolbar and the keyboard is released.
    pub fn enter_countdown<M: 'static>(
        countdown: &crate::capture::countdown::Countdown,
    ) -> cosmic::iced::Task<M> {
        use cosmic::iced::platform_specific::shell::commands::layer_surface::{
            set_input_zone, set_keyboard_interactivity,
        };

        cosmic::Task::batch(countdown.windows.iter().flat_map(|w| {
            [
                set_input_zone(w.id, Some(vec![w.input_zone()])),
                set_keyboard_interactivity(w.id, wlr_layer::KeyboardInteractivity::None),
            ]
        }))
    }

    /// Hand the selected output's overlay to the recording chrome. The others stay
    /// mapped, empty and click-through.
    pub fn enter_recording<M: 'static>(&self) -> cosmic::iced::Task<M> {
        use cosmic::iced::platform_specific::shell::commands::layer_surface::{
            set_input_zone, set_keyboard_interactivity,
        };

        let Some(indicator) = self.recording_indicator.as_ref() else {
            return cosmic::Task::none();
        };

        cosmic::Task::batch(self.outputs.iter().flat_map(|output| {
            let input_zone = if output.id == indicator.chrome_window_id {
                indicator.chrome_input_zones()
            } else {
                Vec::new()
            };
            [
                set_input_zone(output.id, Some(input_zone)),
                set_keyboard_interactivity(output.id, wlr_layer::KeyboardInteractivity::None),
            ]
        }))
    }

    /// Drive the countdown. Lives on `App`, since firing, cancelling and dropping the pill
    /// reach the session and the overlay surfaces.
    fn update_countdown(&mut self, msg: CountdownMsg) -> cosmic::iced::Task<cosmic::Action<Msg>> {
        match msg {
            CountdownMsg::Refresh(started) => {
                // Redraw for the delay numeral, scheduled against the deadline.
                match self.countdown.as_ref() {
                    Some(c) if c.started == started && !c.is_over(Instant::now()) => {
                        countdown_refresh(c).map(cosmic::Action::App)
                    }
                    _ => cosmic::iced::Task::none(),
                }
            }
            CountdownMsg::ToolbarFrame(now) => {
                // Stops the frame subscription. The input zone follows the
                // toolbar's size through `Measured`.
                if let Some(countdown) = self.countdown.as_mut()
                    && now.saturating_duration_since(countdown.started)
                        >= crate::capture::countdown::TOOLBAR_TRANSITION_DURATION
                {
                    countdown.toolbar_transition_finished = true;
                }
                cosmic::iced::Task::none()
            }
            CountdownMsg::Fire(started) => {
                // A cancelled countdown's timer still fires. Match on the start instant.
                let Some(countdown) = self
                    .countdown
                    .as_mut()
                    .filter(|c| c.started == started && !c.fired)
                else {
                    return cosmic::iced::Task::none();
                };

                // The pill is capture-excluded, so it stays up through the capture.
                countdown.fired = true;

                let helper = self.wayland_helper.clone();
                // The session's setting, since the config write may lag.
                let show_cursor = self
                    .capture
                    .as_ref()
                    .is_some_and(|capture| capture.ui.show_cursor);
                cosmic::iced::Task::perform(
                    async move { capture_all_outputs(helper.clone(), show_cursor).await },
                    |images| cosmic::Action::App(Msg::DelayedCaptureReady(images)),
                )
            }
            CountdownMsg::Cancel => {
                if self.countdown.take().is_none() {
                    return cosmic::iced::Task::none();
                }

                // Nothing was captured: the session and the armed delay are untouched.
                self.leave_countdown()
            }
            CountdownMsg::DragStart(id) => {
                if let Some(countdown) = self.countdown.as_mut() {
                    countdown.dragging = Some(id);
                    // Resolved on the first motion, which is the first point at
                    // which both the cursor and the toolbar's corner are known.
                    countdown.drag_offset = None;
                }
                cosmic::iced::Task::none()
            }
            CountdownMsg::DragMove(x, y) => {
                if let Some(countdown) = self.countdown.as_mut()
                    && let Some(id) = countdown.dragging
                {
                    let offset = countdown.drag_offset;
                    if let Some(window) = countdown.windows.iter_mut().find(|w| w.id == id) {
                        let (left, top) = window.origin();
                        let (dx, dy) = offset.unwrap_or((x - left, y - top));
                        countdown.drag_offset = Some((dx, dy));
                        window.move_to((x - dx, y - dy));
                    }
                }
                cosmic::iced::Task::none()
            }
            CountdownMsg::Measured(id, w, h) => {
                let Some(countdown) = self.countdown.as_mut() else {
                    return cosmic::iced::Task::none();
                };
                let dragging = countdown.dragging.is_some();
                let Some(window) = countdown.windows.iter_mut().find(|w| w.id == id) else {
                    return cosmic::iced::Task::none();
                };
                window.resize((w, h));
                // Mid-drag the zone follows on release.
                if dragging {
                    return cosmic::iced::Task::none();
                }
                cosmic::iced::platform_specific::shell::commands::layer_surface::set_input_zone(
                    id,
                    Some(vec![window.input_zone()]),
                )
            }
            CountdownMsg::DragEnd => {
                let Some(countdown) = self.countdown.as_mut() else {
                    return cosmic::iced::Task::none();
                };
                let Some(id) = countdown.dragging.take() else {
                    return cosmic::iced::Task::none();
                };
                countdown.drag_offset = None;

                // Move the input region with the toolbar.
                let Some(window) = countdown.windows.iter().find(|w| w.id == id) else {
                    return cosmic::iced::Task::none();
                };
                let moved =
                    cosmic::iced::platform_specific::shell::commands::layer_surface::set_input_zone(
                        window.id,
                        Some(vec![window.input_zone()]),
                    );
                let (output_name, (x, y)) = (window.output_name.clone(), window.origin());

                // The capture toolbar reopens where this one was left.
                if let Some(capture) = self.capture.as_mut() {
                    _ = capture
                        .ui
                        .toolbar_pos
                        .insert(output_name, cosmic::iced::core::Point::new(x, y));
                }

                moved
            }
        }
    }

    /// Give the overlay back to the selection UI after a countdown.
    fn leave_countdown<M: 'static>(&self) -> cosmic::iced::Task<M> {
        use cosmic::iced::platform_specific::shell::commands::layer_surface::{
            set_input_zone, set_keyboard_interactivity,
        };

        cosmic::Task::batch(self.outputs.iter().flat_map(|o| {
            [
                // `None` gives the whole output to the selection UI. A drag across
                // the output depends on that.
                set_input_zone(o.id, None),
                set_keyboard_interactivity(o.id, wlr_layer::KeyboardInteractivity::Exclusive),
            ]
        }))
    }
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;

    type Flags = AppFlags;

    type Message = Msg;

    const APP_ID: &'static str = "io.github.hojjatabdollahi.snappea";

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn init(
        mut core: app::Core,
        flags: Self::Flags,
    ) -> (Self, cosmic::iced::Task<cosmic::Action<Self::Message>>) {
        let wayland_conn = wayland_client::Connection::connect_to_env().unwrap();
        let wayland_helper = crate::wayland::WaylandHelper::new(wayland_conn);
        let dummy_id = window::Id::unique();

        core.set_auto_blur(Default::default());

        (
            Self {
                core,
                capture: Default::default(),
                outputs: Default::default(),
                wayland_helper,
                tx: None,
                qr_scan: None,
                color_picker: cosmic::widget::ColorPickerModel::new(
                    fl!("hex"),
                    fl!("rgb"),
                    None,
                    Some(cosmic::iced::Color::BLACK),
                )
                .width(cosmic::iced::Length::Fixed(248.0))
                .height(cosmic::iced::Length::Fixed(148.0)),
                text: None,
                font_families: viewer_tools::annotate::font_families(),
                text_style_model: crate::capture::text::text_style_model(),
                text_align_model: crate::capture::text::text_align_model(),
                countdown: None,
                recording_indicator: None,
                direct_screenshot: flags.direct_screenshot,
                screenshot_windows_pending: false,
                dummy_id,
            },
            dummy_surface(dummy_id),
        )
    }

    fn view(&self) -> cosmic::Element<'_, Self::Message> {
        unimplemented!()
    }

    fn view_window(&self, id: window::Id) -> cosmic::Element<'_, Self::Message> {
        if id == self.dummy_id {
            return cosmic::iced::widget::Space::new()
                .width(cosmic::iced::core::Length::Fill)
                .height(cosmic::iced::core::Length::Fill)
                .into();
        }

        // We handle countdown first, since it shares id with the selection UI
        if let Some(countdown) = self.countdown.as_ref().filter(|c| c.owns(id)) {
            return crate::capture::countdown::view(
                countdown,
                id,
                Instant::now(),
                crate::capture::countdown::Handlers {
                    on_drag_start: Msg::Countdown(CountdownMsg::DragStart(id)),
                    on_drag_end: Msg::Countdown(CountdownMsg::DragEnd),
                    on_drag_move: Box::new(|x, y| Msg::Countdown(CountdownMsg::DragMove(x, y))),
                    on_resize: Box::new(move |w, h| {
                        Msg::Countdown(CountdownMsg::Measured(id, w, h))
                    }),
                    on_transition_noop: Msg::Ignore,
                    on_cancel: Msg::Countdown(CountdownMsg::Cancel),
                },
            );
        }

        if let Some(indicator) = &self.recording_indicator {
            // Toolbar, popup and the blinking region border, kept out of the recording
            if indicator.chrome_window_id == id {
                return render_recording_chrome(indicator);
            }
            // Live annotations, which are meant to be in the recording
            if indicator.video_live_annotation_window_id == id {
                return render_recording_annotations(indicator);
            }
        } else if self.outputs.iter().any(|o| o.id == id) {
            // if no countdown, and no active recording, then show the default UI
            return crate::capture::view::view(self, id).map(Msg::Screenshot);
        }

        // Other overlays (or unknown surfaces) stay mapped, empty and click-through.
        cosmic::iced::widget::space()
            .width(cosmic::iced::core::Length::Fixed(1.0))
            .into()
    }

    fn update(
        &mut self,
        message: Self::Message,
    ) -> cosmic::iced::Task<cosmic::Action<Self::Message>> {
        match message {
            Msg::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                // A label being typed takes every key.
                if self.text.is_some() {
                    return self.update(Msg::Screenshot(crate::capture::msg::Msg::text(
                        crate::capture::msg::TextAction::Key(
                            key,
                            modifiers,
                            text.map(|t| t.to_string()),
                        ),
                    )));
                }
                if let Some(capture) = self.capture.as_ref() {
                    let focused_output_index = capture.selection.focused_output_index;
                    if let Some(msg) = crate::capture::shortcuts::handle_key_event(
                        capture,
                        key,
                        modifiers,
                        focused_output_index,
                    ) {
                        return self.update(Msg::Screenshot(msg));
                    }
                }
                cosmic::iced::Task::none()
            }
            Msg::Keyboard(_) => cosmic::iced::Task::none(),
            Msg::Portal(e) => match e {
                crate::dbus::Event::Screenshot(capture) => {
                    crate::capture::flow::start(self, capture).map(cosmic::Action::App)
                }
                crate::dbus::Event::Init(tx) => {
                    self.tx = Some(tx);
                    Task::none()
                }
                crate::dbus::Event::RecordingStopped => {
                    // Recording was stopped via PrintScreen key: clean up UI
                    // This reuses the existing RecordingStopped handler
                    self.update(Msg::RecordingStopped)
                }
            },
            Msg::Screenshot(m) => {
                crate::capture::update::update_msg(self, m).map(cosmic::Action::App)
            }
            Msg::Ignore => cosmic::iced::Task::none(),
            Msg::RecordingStopped => {
                // Clean up capture and send Cancelled to portal
                if let Some(capture) = self.capture.take() {
                    let tx = capture.portal.tx;
                    tokio::spawn(async move {
                        let _ = tx.send(crate::dbus::PortalResponse::Cancelled).await;
                    });
                }

                if let Some(mut indicator) = self.recording_indicator.take() {
                    log::info!("Recording stopped, closing recording surfaces");
                    return indicator.destroy_surfaces(&self.outputs);
                }
                cosmic::iced::Task::none()
            }
            Msg::Recording(m) => match &mut self.recording_indicator {
                Some(indicator) => indicator.update(m),
                None => cosmic::iced::Task::none(),
            },
            Msg::Control(cmd) => {
                match cmd {
                    ControlCommand::TakeScreenshot => {
                        log::info!("Control: Take screenshot requested");
                        if let Some(tx) = &self.tx {
                            let tx = tx.clone();
                            let helper = self.wayland_helper.clone();
                            tokio::spawn(async move {
                                // Trigger a new screenshot by sending the appropriate event
                                // This simulates what would happen on PrintScreen
                                trigger_screenshot(helper, tx).await;
                            });
                        }
                    }
                    ControlCommand::ToggleRecording => {
                        log::info!("Control: Toggle recording requested");
                        if crate::recording::is_recording() {
                            // Stop recording
                            return self.update(Msg::Screenshot(crate::capture::msg::Msg::Action(
                                crate::capture::msg::ActionMsg::StopRecording,
                            )));
                        }
                    }
                    ControlCommand::Quit => {
                        log::info!("Control: Quit requested");
                        std::process::exit(0);
                    }
                }
                cosmic::iced::Task::none()
            }
            Msg::LayerClosed(id) if id == self.dummy_id => {
                log::warn!(
                    "Dummy layer surface was closed by compositor (output removed?), re-creating it"
                );
                self.dummy_id = window::Id::unique();
                dummy_surface(self.dummy_id)
            }
            Msg::LayerClosed(_) => cosmic::iced::Task::none(),
            Msg::RetryPendingWindows => {
                // OutputEvent::Created never fired (outputs were advertised before the
                // subscription). Create the windows now.
                if self.screenshot_windows_pending && !self.outputs.is_empty() {
                    log::warn!(
                        "Failsafe: creating screenshot windows after deferred creation \
                         timed out ({} outputs available)",
                        self.outputs.len()
                    );
                    self.screenshot_windows_pending = false;
                    return self.open_overlay();
                } else if self.screenshot_windows_pending {
                    // Still no outputs. Log and give up instead of hanging.
                    log::error!(
                        "Failsafe: screenshot_windows_pending is true but outputs list \
                         is still empty after 500 ms, so overlay windows cannot be created. \
                         The compositor did not advertise any outputs."
                    );
                    self.screenshot_windows_pending = false;
                }
                cosmic::iced::Task::none()
            }
            Msg::Countdown(m) => self.update_countdown(m),
            Msg::DelayedCaptureReady(output_images) => {
                // The delay elapsed: swap the fresh pixels in under the same selection and reopen the overlay.
                let Some(capture) = self.capture.as_mut() else {
                    log::warn!("DelayedCaptureReady with no capture in progress");
                    return cosmic::iced::Task::none();
                };
                if output_images.is_empty() {
                    log::error!("Delayed capture produced no images; aborting recapture");
                    return cosmic::iced::Task::none();
                }
                capture.output_images = output_images;
                capture.annotations = crate::capture::AnnotationState::default();
                capture.detection = crate::capture::DetectionState::default();
                capture.selection.has_mouse_entered = false;
                capture.ui.now = Instant::now();

                // A delay fires once.
                capture.ui.capture_delay_secs = 0;
                crate::config::Config::store("capture_delay_secs", 0u32);

                if self.outputs.is_empty() {
                    log::warn!("Delayed capture ready but no outputs known; deferring windows");
                    self.countdown = None;
                    self.screenshot_windows_pending = true;
                    return cosmic::iced::Task::none();
                }

                // Dropping the countdown swaps the overlay back from the pill.
                self.countdown = None;
                self.leave_countdown()
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
                        });

                        // If screenshot capture arrived before outputs were ready, create
                        // the overlay window now that we have our first output.
                        if self.screenshot_windows_pending
                            && let Some(output_state) = self.outputs.last_mut()
                        {
                            output_state.id = window::Id::unique();
                            let surface_cmd = overlay_surface(output_state);
                            log::info!(
                                "Deferred screenshot window creation triggered for output {:?}",
                                output_state.name
                            );
                            self.screenshot_windows_pending = false;
                            return surface_cmd;
                        }
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
                            log::warn!("Updated output {wl_output:?} not found");
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
                            });
                        }
                    }
                    e => {
                        log::warn!("Unhandled output event: {wl_output:?} {e:?}");
                    }
                }

                cosmic::iced::Task::none()
            }
        }
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        // Use direct screenshot subscription if in direct mode, otherwise portal subscription
        let screenshot_sub = if self.direct_screenshot {
            direct_screenshot_subscription(self.wayland_helper.clone()).map(|e| match e {
                PortalEvent::Screenshot(se) => Msg::Portal(se),
                PortalEvent::Control(cmd) => Msg::Control(cmd),
            })
        } else {
            portal_subscription(self.wayland_helper.clone()).map(|e| match e {
                PortalEvent::Screenshot(se) => Msg::Portal(se),
                PortalEvent::Control(cmd) => Msg::Control(cmd),
            })
        };

        let mut subscriptions = vec![
            screenshot_sub,
            listen_with(|e, _, _| match e {
                cosmic::iced::core::Event::PlatformSpecific(
                    cosmic::iced::core::event::PlatformSpecific::Wayland(
                        cosmic::iced::core::event::wayland::Event::Output(o_event, wl_output),
                    ),
                ) => Some(Msg::Output(o_event, wl_output)),
                cosmic::iced::core::Event::PlatformSpecific(
                    cosmic::iced::core::event::PlatformSpecific::Wayland(
                        cosmic::iced::core::event::wayland::Event::Layer(
                            cosmic::iced::core::event::wayland::LayerEvent::Done,
                            _surface,
                            id,
                        ),
                    ),
                ) => Some(Msg::LayerClosed(id)),
                cosmic::iced::core::Event::Keyboard(keyboard_event) => {
                    Some(Msg::Keyboard(keyboard_event))
                }
                _ => None,
            }),
        ];

        // Add timers and event listeners when recording indicator is active
        if let Some(indicator) = self.recording_indicator.as_ref() {
            // Blink timer (500ms), only while there is a border to blink
            if indicator.region_border {
                subscriptions.push(
                    cosmic::iced::time::every(std::time::Duration::from_millis(500))
                        .map(|_| Msg::Recording(RecordingMsg::Blink)),
                );
            }

            // Check if recording is still active (every second)
            subscriptions.push(
                cosmic::iced::time::every(std::time::Duration::from_millis(1000)).map(|_| {
                    if crate::recording::is_recording() {
                        Msg::Recording(RecordingMsg::Blink) // Keep indicator alive (noop blink)
                    } else {
                        Msg::RecordingStopped
                    }
                }),
            );

            // Annotation fade timer (50ms for smooth fading), only while there is something to
            // fade. Otherwise it would repaint the full-output overlay 20 times a second with no change.
            if self.recording_indicator.as_ref().is_some_and(|indicator| {
                !indicator.annotations.is_empty() || indicator.current_stroke.is_some()
            }) {
                subscriptions.push(
                    cosmic::iced::time::every(std::time::Duration::from_millis(50))
                        .map(|_| Msg::Recording(RecordingMsg::AnnotationFade)),
                );
            }

            if self
                .recording_indicator
                .as_ref()
                .is_some_and(|indicator| !indicator.toolbar_transition_finished)
            {
                subscriptions.push(
                    cosmic::iced::window::frames()
                        .map(|(_, instant)| Msg::Recording(RecordingMsg::ToolbarFrame(instant))),
                );
            }
        }

        // The shutter button runs its own countdown clock. This timer only covers the handoff animation.
        if self
            .countdown
            .as_ref()
            .is_some_and(|countdown| !countdown.toolbar_transition_finished)
        {
            subscriptions.push(
                cosmic::iced::window::frames()
                    .map(|(_, instant)| Msg::Countdown(CountdownMsg::ToolbarFrame(instant))),
            );
        }

        // Frame clock for the toolbar animation. Polls slowly once settled so `sync` still notices changes.
        if let Some(capture) = self.capture.as_ref() {
            if capture.ui.toolbar_anim.is_animating(capture.ui.now) {
                subscriptions.push(cosmic::iced::window::frames().map(|(window_id, instant)| {
                    Msg::Screenshot(crate::capture::msg::Msg::timeline_tick(window_id, instant))
                }));
            } else {
                subscriptions.push(
                    cosmic::iced::time::every(std::time::Duration::from_millis(50)).map(|_| {
                        Msg::Screenshot(crate::capture::msg::Msg::timeline_tick(
                            cosmic::iced::window::Id::NONE,
                            std::time::Instant::now(),
                        ))
                    }),
                );
            }
        }

        // Failsafe: retry window creation if OutputEvent::Created never comes.
        if self.screenshot_windows_pending {
            subscriptions.push(
                cosmic::iced::time::every(std::time::Duration::from_millis(500))
                    .map(|_| Msg::RetryPendingWindows),
            );
        }

        Subscription::batch(subscriptions)
    }
}

/// Events from the portal subscription (includes both screenshot and control events)
pub enum PortalEvent {
    Screenshot(crate::dbus::Event),
    Control(ControlCommand),
}

pub enum SubscriptionState {
    Init,
    Waiting(
        zbus::Connection,
        tokio::sync::mpsc::Receiver<crate::dbus::Event>,
        tokio::sync::mpsc::Receiver<ControlCommand>,
    ),
}

pub fn portal_subscription(
    helper: crate::wayland::WaylandHelper,
) -> cosmic::iced::Subscription<PortalEvent> {
    #[derive(Clone)]
    struct PortalSubscription(crate::wayland::WaylandHelper);

    impl std::hash::Hash for PortalSubscription {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            TypeId::of::<Self>().hash(state);
        }
    }

    Subscription::run_with(PortalSubscription(helper), |data| {
        let helper = data.0.clone();

        cosmic::iced::stream::channel(
            10,
            move |mut output: cosmic::iced::futures::channel::mpsc::Sender<PortalEvent>| async move {
                let mut state = SubscriptionState::Init;
                loop {
                    if let Err(err) = process_changes(&mut state, &mut output, &helper).await {
                        log::debug!("Portal Subscription Error: {err:?}");
                        futures::future::pending::<()>().await;
                    }
                }
            },
        )
    })
}

pub async fn process_changes(
    state: &mut SubscriptionState,
    output: &mut futures::channel::mpsc::Sender<PortalEvent>,
    wayland_helper: &crate::wayland::WaylandHelper,
) -> anyhow::Result<()> {
    match state {
        SubscriptionState::Init => {
            let (tx, rx) = tokio::sync::mpsc::channel(10);
            let (ctrl_tx, ctrl_rx) = tokio::sync::mpsc::channel(10);

            let connection = zbus::connection::Builder::session()?
                .name(DBUS_NAME)?
                .serve_at(
                    DBUS_PATH,
                    crate::dbus::Screenshot::new(wayland_helper.clone(), tx.clone()),
                )?
                .serve_at(CONTROL_PATH, ControlInterface::new(ctrl_tx))?
                .build()
                .await?;
            log::info!(
                "D-Bus interfaces registered: portal at {DBUS_PATH}, control at {CONTROL_PATH}"
            );
            _ = output
                .send(PortalEvent::Screenshot(crate::dbus::Event::Init(tx)))
                .await;
            *state = SubscriptionState::Waiting(connection, rx, ctrl_rx);
        }
        SubscriptionState::Waiting(_conn, rx, ctrl_rx) => loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    match event {
                        crate::dbus::Event::Screenshot(capture) => {
                            if let Err(err) = output.send(PortalEvent::Screenshot(crate::dbus::Event::Screenshot(capture))).await {
                                log::error!("Error sending screenshot event: {err:?}");
                            }
                        }
                        crate::dbus::Event::RecordingStopped => {
                            if let Err(err) = output.send(PortalEvent::Screenshot(crate::dbus::Event::RecordingStopped)).await {
                                log::error!("Error sending RecordingStopped event: {err:?}");
                            }
                        }
                        crate::dbus::Event::Init(_) => {}
                    }
                }
                Some(cmd) = ctrl_rx.recv() => {
                    if let Err(err) = output.send(PortalEvent::Control(cmd)).await {
                        log::error!("Error sending control command: {err:?}");
                    }
                }
            }
        },
    }
    Ok(())
}

/// Subscription for direct screenshot mode (no D-Bus portal): captures the
/// screens at startup, hands the UI a session, then relays control commands.
pub fn direct_screenshot_subscription(
    helper: crate::wayland::WaylandHelper,
) -> cosmic::iced::Subscription<PortalEvent> {
    #[derive(Clone)]
    struct DirectScreenshotSubscription(crate::wayland::WaylandHelper);

    impl std::hash::Hash for DirectScreenshotSubscription {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            TypeId::of::<Self>().hash(state);
        }
    }

    Subscription::run_with(DirectScreenshotSubscription(helper), |data| {
        let helper = data.0.clone();

        cosmic::iced::stream::channel(
            10,
            move |mut output: cosmic::iced::futures::channel::mpsc::Sender<PortalEvent>| async move {
                let (ctrl_tx, mut ctrl_rx) = tokio::sync::mpsc::channel::<ControlCommand>(10);
                // Held for the life of the subscription. No portal interface in direct mode.
                let _connection = match register_control(ctrl_tx).await {
                    Ok(connection) => Some(connection),
                    Err(err) => {
                        log::warn!("Failed to register the D-Bus control interface: {err}");
                        None
                    }
                };

                // Nothing waits on a direct-mode capture, so the receiver is dropped at once.
                let (init_tx, _) = tokio::sync::mpsc::channel(1);
                _ = output
                    .send(PortalEvent::Screenshot(crate::dbus::Event::Init(init_tx)))
                    .await;

                // Small delay to let the app initialize
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                if let Some(capture) = fresh_capture(&helper).await
                    && let Err(e) = output
                        .send(PortalEvent::Screenshot(crate::dbus::Event::Screenshot(
                            capture,
                        )))
                        .await
                {
                    log::error!("Failed to send direct screenshot event: {e}");
                }

                // Keep the subscription alive and relay control commands.
                while let Some(cmd) = ctrl_rx.recv().await {
                    if output.send(PortalEvent::Control(cmd)).await.is_err() {
                        break;
                    }
                }
            },
        )
    })
}

/// Serve the control interface on the session bus.
async fn register_control(
    ctrl_tx: tokio::sync::mpsc::Sender<ControlCommand>,
) -> zbus::Result<zbus::Connection> {
    let connection = zbus::connection::Builder::session()?
        .name(DBUS_NAME)?
        .serve_at(CONTROL_PATH, ControlInterface::new(ctrl_tx))?
        .build()
        .await?;
    log::info!("D-Bus control interface registered at {CONTROL_PATH}");
    Ok(connection)
}

/// Capture every output into a name-keyed image map.
pub async fn capture_all_outputs(
    helper: crate::wayland::WaylandHelper,
    show_cursor: bool,
) -> std::collections::HashMap<String, crate::capture::ScreenshotImage> {
    use crate::capture::ScreenshotImage;
    use crate::wayland::CaptureSource;
    use std::collections::HashMap;

    let outputs: Vec<_> = helper
        .outputs()
        .into_iter()
        .filter_map(|wl_output| {
            let info = helper.output_info(&wl_output)?;
            Some((wl_output, info.name?))
        })
        .collect();

    let mut output_images = HashMap::new();
    for (wl_output, name) in &outputs {
        if let Some(frame) = helper
            .capture_source_shm(CaptureSource::Output(wl_output.clone()), show_cursor)
            .await
        {
            match ScreenshotImage::new(frame) {
                Ok(img) => {
                    output_images.insert(name.clone(), img);
                }
                Err(e) => log::error!("Failed to create screenshot image for {name}: {e}"),
            }
        } else {
            log::error!("Failed to capture output {name}");
        }
    }
    output_images
}

/// A user-initiated session over the screens as they are now, or `None` when
/// no output could be captured.
async fn fresh_capture(helper: &crate::wayland::WaylandHelper) -> Option<crate::capture::Capture> {
    use crate::capture::{Capture, PortalContext};

    // No session yet, so the setting comes from where it is kept.
    let config = crate::config::Config::load();
    let output_images = capture_all_outputs(helper.clone(), config.show_cursor).await;
    if output_images.is_empty() {
        log::error!("No outputs could be captured");
        return None;
    }
    // Nothing waits on the response, so the receiver is dropped at once.
    let (tx, _) = tokio::sync::mpsc::channel(1);
    let portal = PortalContext {
        tx,
        expects_response: false,
    };
    Some(Capture::new(portal, output_images, &config))
}

/// Take a screenshot on a D-Bus control command.
async fn trigger_screenshot(
    helper: crate::wayland::WaylandHelper,
    tx: tokio::sync::mpsc::Sender<crate::dbus::Event>,
) {
    if let Some(capture) = fresh_capture(&helper).await
        && let Err(e) = tx.send(crate::dbus::Event::Screenshot(capture)).await
    {
        log::error!("Failed to send screenshot event: {e}");
    }
}

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

/// State for the recording indicator overlay
#[derive(Debug, Clone)]
pub struct RecordingIndicator {
    /// Window ID for the layer surface
    pub window_id: window::Id,
    /// Output name where recording is happening
    pub output_name: String,
    /// Recording region in output-local logical coordinates
    pub region: (i32, i32, u32, u32),
    /// Current blink state (true = visible border)
    pub blink_visible: bool,
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

        // Add blink timer when recording indicator is active
        if self.recording_indicator.is_some() {
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

/// Render the recording indicator overlay - a blinking red border
fn render_recording_indicator(indicator: &RecordingIndicator) -> cosmic::Element<'static, Msg> {
    use cosmic::iced_core::Length;
    use cosmic::iced_widget::canvas::{self, Geometry, Path, Stroke};

    // We need to store region info to use in the closure
    let region = indicator.region;
    let visible = indicator.blink_visible;

    struct RecordingBorder {
        region: (i32, i32, u32, u32),
        visible: bool,
    }

    impl canvas::Program<Msg, cosmic::Theme, cosmic::Renderer> for RecordingBorder {
        type State = ();

        fn draw(
            &self,
            _state: &Self::State,
            renderer: &cosmic::Renderer,
            _theme: &cosmic::Theme,
            bounds: cosmic::iced_core::Rectangle,
            _cursor: cosmic::iced_core::mouse::Cursor,
        ) -> Vec<Geometry> {
            if !self.visible {
                return vec![];
            }

            let mut frame = canvas::Frame::new(renderer, bounds.size());

            // Draw a red border rectangle OUTSIDE the recording region
            // so it's not captured in the recording
            let border_width = 4.0;
            // Position the path so the inner edge of the stroke aligns with the region boundary
            let x = self.region.0 as f32 - border_width / 2.0;
            let y = self.region.1 as f32 - border_width / 2.0;
            let w = self.region.2 as f32 + border_width;
            let h = self.region.3 as f32 + border_width;

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

            vec![frame.into_geometry()]
        }
    }

    let program = RecordingBorder { region, visible };

    canvas::Canvas::new(program)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

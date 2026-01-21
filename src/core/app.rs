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
}

#[derive(Debug, Clone)]
pub struct OutputState {
    pub output: WlOutput,
    pub id: window::Id,
    pub name: String,
    pub logical_size: (u32, u32),
    pub logical_pos: (i32, i32),
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
        let subscriptions = vec![
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

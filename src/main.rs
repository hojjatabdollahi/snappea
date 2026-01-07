mod annotations;
mod buffer;
mod capture;
mod config;
mod core;
mod domain;
mod localize;
mod render;
mod screenshot;
mod session;
mod wayland;
mod widget;

fn main() -> cosmic::iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    localize::localize();
    core::app::run()
}

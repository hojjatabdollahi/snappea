use std::collections::HashMap;
use zbus::zvariant;

mod app;
mod arrow;
mod buffer;
mod localize;
mod ocr;
mod qr;
mod screenshot;
mod wayland;
mod widget;

static DBUS_NAME: &str = "org.freedesktop.impl.portal.blazingshot";
static DBUS_PATH: &str = "/org/freedesktop/portal/desktop";

const PORTAL_RESPONSE_SUCCESS: u32 = 0;
const PORTAL_RESPONSE_CANCELLED: u32 = 1;
const PORTAL_RESPONSE_OTHER: u32 = 2;

#[derive(zvariant::Type)]
#[zvariant(signature = "(ua{sv})")]
pub enum PortalResponse<T: zvariant::Type + serde::Serialize> {
    Success(T),
    Cancelled,
    Other,
}

impl<T: zvariant::Type + serde::Serialize> serde::Serialize for PortalResponse<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Success(res) => (PORTAL_RESPONSE_SUCCESS, res).serialize(serializer),
            Self::Cancelled => (
                PORTAL_RESPONSE_CANCELLED,
                HashMap::<String, zvariant::Value>::new(),
            )
                .serialize(serializer),
            Self::Other => (
                PORTAL_RESPONSE_OTHER,
                HashMap::<String, zvariant::Value>::new(),
            )
                .serialize(serializer),
        }
    }
}

fn main() -> cosmic::iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    localize::localize();
    app::run()
}

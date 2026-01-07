//! D-Bus Portal interface types
//!
//! This module contains types for XDG Desktop Portal integration.

use std::collections::HashMap;
use zbus::zvariant;

/// Portal response status codes
pub const PORTAL_RESPONSE_SUCCESS: u32 = 0;
pub const PORTAL_RESPONSE_CANCELLED: u32 = 1;
pub const PORTAL_RESPONSE_OTHER: u32 = 2;

/// Portal response wrapper for D-Bus responses
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

/// D-Bus service name for the portal
pub const DBUS_NAME: &str = "org.freedesktop.impl.portal.blazingshot";

/// D-Bus object path for the portal
pub const DBUS_PATH: &str = "/org/freedesktop/portal/desktop";

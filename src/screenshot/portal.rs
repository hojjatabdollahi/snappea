//! D-Bus portal types for screenshot functionality
//!
//! This module contains the zvariant types used for the freedesktop portal interface.

use zbus::zvariant;

/// Options passed from the portal request
#[derive(zvariant::DeserializeDict, zvariant::Type, Clone, Debug)]
#[zvariant(signature = "a{sv}")]
pub struct ScreenshotOptions {
    pub modal: Option<bool>,
    pub interactive: Option<bool>,
    pub choose_destination: Option<bool>,
}

/// Result returned from a successful screenshot
#[derive(zvariant::SerializeDict, zvariant::Type)]
#[zvariant(signature = "a{sv}")]
pub struct ScreenshotResult {
    pub uri: String,
}

impl ScreenshotResult {
    pub fn new(uri: String) -> Self {
        Self { uri }
    }
}

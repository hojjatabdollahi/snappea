//! D-Bus control interface for single-instance support
//!
//! This module provides a D-Bus interface that allows external processes
//! to communicate with a running SnapPea instance.

use tokio::sync::mpsc;

/// D-Bus object path for the control interface
pub const CONTROL_PATH: &str = "/io/github/hojjatabdollahi/snappea";

/// Commands that can be sent to a running instance
#[derive(Debug, Clone)]
pub enum ControlCommand {
    /// Take a screenshot (opens the selection UI)
    TakeScreenshot,
    /// Toggle recording (stop if recording, otherwise no-op for now)
    ToggleRecording,
    /// Quit the application
    Quit,
}

/// D-Bus control interface
pub struct ControlInterface {
    tx: mpsc::Sender<ControlCommand>,
}

impl ControlInterface {
    pub fn new(tx: mpsc::Sender<ControlCommand>) -> Self {
        Self { tx }
    }
}

#[zbus::interface(name = "io.github.hojjatabdollahi.snappea.Control")]
impl ControlInterface {
    /// Take a screenshot - opens the selection UI
    async fn take_screenshot(&self) -> bool {
        log::info!("D-Bus: TakeScreenshot command received");
        self.tx.send(ControlCommand::TakeScreenshot).await.is_ok()
    }

    /// Toggle recording - stops recording if active
    async fn toggle_recording(&self) -> bool {
        log::info!("D-Bus: ToggleRecording command received");
        self.tx.send(ControlCommand::ToggleRecording).await.is_ok()
    }

    /// Quit the application
    async fn quit(&self) -> bool {
        log::info!("D-Bus: Quit command received");
        self.tx.send(ControlCommand::Quit).await.is_ok()
    }

    /// Check if the application is running (always returns true if reachable)
    async fn ping(&self) -> bool {
        true
    }

    /// Check if currently recording
    async fn is_recording(&self) -> bool {
        crate::screencast::is_recording()
    }
}

/// Check if another instance is running by trying to call Ping on the D-Bus interface
pub async fn is_instance_running() -> bool {
    let Ok(connection) = zbus::Connection::session().await else {
        return false;
    };

    // Try to call Ping method on the control interface
    let result = connection
        .call_method(
            Some(super::portal::DBUS_NAME),
            CONTROL_PATH,
            Some("io.github.hojjatabdollahi.snappea.Control"),
            "Ping",
            &(),
        )
        .await;

    result.is_ok()
}

/// Send a command to the running instance
pub async fn send_command(command: &str) -> Result<bool, zbus::Error> {
    let connection = zbus::Connection::session().await?;

    let method = match command {
        "screenshot" => "TakeScreenshot",
        "toggle-recording" => "ToggleRecording",
        "quit" => "Quit",
        _ => "TakeScreenshot", // Default to screenshot
    };

    let reply: bool = connection
        .call_method(
            Some(super::portal::DBUS_NAME),
            CONTROL_PATH,
            Some("io.github.hojjatabdollahi.snappea.Control"),
            method,
            &(),
        )
        .await?
        .body()
        .deserialize()?;

    Ok(reply)
}

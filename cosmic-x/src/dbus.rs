// SPDX-License-Identifier: GPL-3.0-only

//! D-Bus: the Screenshot portal, the control interface and notifications.

use crate::capture::AnnotationState;
use crate::capture::Capture;
use crate::capture::DetectionState;
use crate::capture::PortalContext;
use crate::capture::ScreenshotImage;
use crate::capture::Selection;
use crate::capture::UiState;
use crate::capture::detect::is_tesseract_available;
use crate::capture::flow::{get_img_path, img_file_name, save_rgba, stitch_outputs};
use crate::config::{Config, SaveLocationChoice};
use crate::geometry::{Choice, DragState, ImageSaveLocation, Rect};
use crate::wayland::CaptureSource;
use crate::wayland::WaylandHelper;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use viewer_tools::annotate::TextFormat;
use wayland_client::protocol::wl_output::WlOutput;
use zbus::zvariant;
use zbus::zvariant::Value;

// XDG Desktop Portal types.

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
pub const DBUS_NAME: &str = "com.system76.CosmicX";

/// D-Bus object path for the portal
pub const DBUS_PATH: &str = "/org/freedesktop/portal/desktop";

// zvariant types for the Screenshot portal.

/// Options a portal caller passes to `Screenshot`.
#[derive(zvariant::DeserializeDict, zvariant::Type, Clone, Debug, Default)]
#[zvariant(signature = "a{sv}")]
pub struct ScreenshotOptions {
    /// Whether to show the selection UI. Off means capture every screen at once.
    /// The spec's `modal` option is ignored, since the selection UI is a layer
    /// surface with no parent window to be modal to.
    pub interactive: Option<bool>,
}

/// Result returned from a successful screenshot
#[derive(zvariant::SerializeDict, zvariant::Type)]
#[zvariant(signature = "a{sv}")]
pub struct ScreenshotResult {
    pub uri: String,
}

/// Result returned from a successful `PickColor`.
#[derive(zvariant::SerializeDict, zvariant::Type)]
#[zvariant(signature = "a{sv}")]
pub struct PickColorResult {
    /// Red, green and blue in 0.0 to 1.0.
    pub color: (f64, f64, f64),
}

// D-Bus control interface for talking to a running instance.

/// D-Bus object path for the control interface
pub const CONTROL_PATH: &str = "/com/system76/CosmicX";

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
    pub const fn new(tx: mpsc::Sender<ControlCommand>) -> Self {
        Self { tx }
    }
}

#[zbus::interface(name = "com.system76.CosmicX.Control")]
impl ControlInterface {
    /// Take a screenshot: opens the selection UI
    async fn take_screenshot(&self) -> bool {
        log::info!("D-Bus: TakeScreenshot command received");
        self.tx.send(ControlCommand::TakeScreenshot).await.is_ok()
    }

    /// Toggle recording: stops recording if active
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
        crate::recording::is_recording()
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
            Some(DBUS_NAME),
            CONTROL_PATH,
            Some("com.system76.CosmicX.Control"),
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
            Some(DBUS_NAME),
            CONTROL_PATH,
            Some("com.system76.CosmicX.Control"),
            method,
            &(),
        )
        .await?
        .body()
        .deserialize()?;

    Ok(reply)
}

// Desktop notifications over `org.freedesktop.Notifications`.

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, &Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Show an error notification. Fire-and-forget on its own thread. Delivery
/// failures are logged, not propagated.
pub fn notify_error(summary: impl Into<String>, body: impl Into<String>) {
    let summary = summary.into();
    let body = body.into();
    std::thread::spawn(move || {
        if let Err(e) = send(&summary, &body) {
            log::warn!("Could not show desktop notification: {e}");
        }
    });
}

fn send(summary: &str, body: &str) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let conn = zbus::Connection::session().await?;
        let proxy = NotificationsProxy::new(&conn).await?;
        let hints: HashMap<&str, &Value<'_>> = HashMap::new();
        proxy
            .notify(
                "COSMIC X",
                0, // replaces_id: 0 = new notification
                "dialog-error",
                summary,
                body,
                &[], // no actions
                hints,
                5000, // expire after 5s
            )
            .await?;
        Ok::<(), zbus::Error>(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sends a real notification. Requires a running notification daemon, so it's
    // ignored by default. Run with: `cargo test notify_smoke -- --ignored`.
    #[test]
    #[ignore = "needs a notification daemon"]
    fn notify_smoke() {
        send("COSMIC X test", "If you can see this, notifications work.")
            .expect("notification should be delivered");
    }
}

pub struct Screenshot {
    wayland_helper: WaylandHelper,
    tx: Sender<Event>,
}

impl Screenshot {
    pub const fn new(wayland_helper: WaylandHelper, tx: Sender<Event>) -> Self {
        Self { wayland_helper, tx }
    }

    async fn capture_outputs(
        &self,
        outputs: &[Output],
    ) -> anyhow::Result<HashMap<String, ScreenshotImage>> {
        let wayland_helper = self.wayland_helper.clone();

        // No session yet, so the setting comes from where it is kept.
        let show_cursor = Config::load().show_cursor;
        let mut map = HashMap::with_capacity(outputs.len());
        for Output {
            output,
            logical_position: (_output_x, _output_y),
            name,
            ..
        } in outputs
        {
            let frame = wayland_helper
                .capture_source_shm(CaptureSource::Output(output.clone()), show_cursor)
                .await
                .ok_or_else(|| anyhow::anyhow!("shm screencopy failed"))?;
            map.insert(name.clone(), ScreenshotImage::new(frame)?);
        }

        Ok(map)
    }

    /// Every screen in one image, saved without showing the UI.
    async fn capture_silently(&self, outputs: &[Output]) -> PortalResponse<ScreenshotResult> {
        let mut images = match self.capture_outputs(outputs).await {
            Ok(images) => images,
            Err(err) => {
                log::error!("Screenshot failed: {err:?}");
                return PortalResponse::Other;
            }
        };
        let frames = outputs
            .iter()
            .filter_map(|o| Some((images.remove(&o.name)?.rgba, o.rect())))
            .collect();
        let Some(image) = stitch_outputs(frames) else {
            log::error!("No output image to save");
            return PortalResponse::Other;
        };
        let Some(path) = configured_save_path() else {
            log::error!("No folder to save the screenshot in");
            return PortalResponse::Other;
        };
        if let Err(err) = save_rgba(&image, &path) {
            log::error!("Failed to save screenshot: {err:?}");
            return PortalResponse::Other;
        }
        match url::Url::from_file_path(&path) {
            Ok(url) => PortalResponse::Success(ScreenshotResult {
                uri: url.to_string(),
            }),
            Err(()) => PortalResponse::Other,
        }
    }
}

/// Where the settings say a new screenshot goes. The clipboard choice names no
/// folder, so a portal caller gets a temporary file.
fn configured_save_path() -> Option<std::path::PathBuf> {
    let config = Config::load();
    match config.save_location {
        SaveLocationChoice::Clipboard => Some(std::env::temp_dir().join(img_file_name())),
        SaveLocationChoice::Documents => get_img_path(ImageSaveLocation::Documents, None),
        SaveLocationChoice::Pictures => get_img_path(ImageSaveLocation::Pictures, None),
        SaveLocationChoice::Custom => {
            get_img_path(ImageSaveLocation::Pictures, Some(&config.custom_save_path))
        }
    }
}

struct Output {
    output: WlOutput,
    logical_position: (i32, i32),
    logical_size: (i32, i32),
    name: String,
}

impl Output {
    const fn rect(&self) -> Rect {
        Rect {
            left: self.logical_position.0,
            top: self.logical_position.1,
            right: self.logical_position.0 + self.logical_size.0,
            bottom: self.logical_position.1 + self.logical_size.1,
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Event {
    Screenshot(Capture),
    Init(Sender<Self>),
    /// Recording was stopped via `PrintScreen`: clean up indicator UI
    RecordingStopped,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl Screenshot {
    async fn screenshot(
        &self,
        #[zbus(connection)] _connection: &zbus::Connection,
        _handle: zvariant::ObjectPath<'_>,
        _app_id: &str,
        _parent_window: &str,
        options: ScreenshotOptions,
    ) -> PortalResponse<ScreenshotResult> {
        // Check if a recording is active: if so, just stop it and return
        // The user needs to press PrintScreen again to get the screenshot UI
        if crate::recording::is_recording() {
            log::info!(
                "Active recording detected, stopping recording (press PrintScreen again for screenshot UI)"
            );
            if let Err(e) = crate::recording::stop_recording() {
                log::error!("Failed to stop recording: {e}");
            }
            // Send event to clean up indicator UI
            if let Err(e) = self.tx.send(Event::RecordingStopped).await {
                log::error!("Failed to send RecordingStopped event: {e}");
            }
            // Return cancelled: the recording was stopped, but no screenshot taken
            // User can press PrintScreen again to get the UI
            return PortalResponse::Cancelled;
        }

        let mut outputs = Vec::new();
        for output in self.wayland_helper.outputs() {
            let Some(info) = self.wayland_helper.output_info(&output) else {
                log::warn!("Output {output:?} has no info");
                continue;
            };
            let Some(name) = info.name.clone() else {
                log::warn!("Output {output:?} has no name");
                continue;
            };
            let Some(logical_position) = info.logical_position else {
                log::warn!("Output {output:?} has no position");
                continue;
            };
            let Some(logical_size) = info.logical_size else {
                log::warn!("Output {output:?} has no size");
                continue;
            };
            log::debug!(
                "Output {}: logical_size={}x{}, scale_factor={}",
                name,
                logical_size.0,
                logical_size.1,
                info.scale_factor
            );
            outputs.push(Output {
                output,
                logical_position,
                logical_size,
                name,
            });
        }
        if outputs.is_empty() {
            log::error!("No output");
            return PortalResponse::Other;
        }

        if !options.interactive.unwrap_or(false) {
            return self.capture_silently(&outputs).await;
        }

        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let output_images = self.capture_outputs(&outputs).await.unwrap_or_default();

        // Log output image sizes for debugging HiDPI
        for (name, img) in &output_images {
            log::debug!(
                "Output image {}: {}x{} pixels",
                name,
                img.rgba.width(),
                img.rgba.height()
            );
        }

        // Ask the compositor which windows can be picked while the screen is
        // still as the capture found it.

        let choice = Choice::Rectangle(Rect::default(), DragState::default());

        // Load persisted config for settings
        let config = Config::load();

        // Send UI immediately with empty QR codes, detection happens async
        if let Err(err) = self
            .tx
            .send(Event::Screenshot(Capture {
                portal: PortalContext {
                    tx,
                    // A real portal caller is blocked on this response awaiting a URI.
                    expects_response: true,
                },
                output_images,
                selection: Selection {
                    choice,
                    location: ImageSaveLocation::Pictures,
                    focused_output_index: 0,
                    also_copy_to_clipboard: false,
                    has_mouse_entered: false,
                },
                detection: DetectionState::default(),
                annotations: AnnotationState::default(),
                ui: {
                    // Start with empty encoders: they will be detected asynchronously
                    // This allows the UI to show immediately without waiting for GStreamer
                    UiState {
                        now: Instant::now(),
                        toolbar_anim: crate::capture::ToolbarAnim::settled(
                            crate::capture::ToolbarSections::of(crate::capture::ToolbarMode {
                                annotating: false,
                                video: false,
                                has_selection: false,
                                annotation_selected: false,
                                can_ocr: false,
                            }),
                        ),
                        settings_drawer_open: false,
                        annotate_mode: false,
                        move_mode: false,
                        delay_popup_open: false,
                        stroke_popup_open: false,
                        font_popup_open: false,
                        primary_shape_tool: config.primary_shape_tool,
                        shape_choice: config
                            .shape_choice
                            .or_first_of(crate::config::ShapeTool::SHAPES),
                        freehand_choice: config
                            .freehand_choice
                            .or_first_of(crate::config::ShapeTool::FREEHAND),
                        shape_popup_open: false,
                        freehand_popup_open: false,
                        text_format: TextFormat::default(),
                        text_format_popup_open: false,
                        shape_color: config.shape_color,
                        shape_thickness: config.shape_thickness,
                        highlighter_thickness: config.highlighter_thickness,
                        text_font_size: config.text_font_size,
                        exit_armed: false,
                        primary_redact_tool: config.primary_redact_tool,
                        redact_popup_open: false,
                        pixelation_block_size: config.pixelation_block_size,
                        recognize_qr_codes: config.recognize_qr_codes,
                        magnifier_popup_open: false,
                        magnifier_magnification: config.magnifier_magnification,
                        capture_delay_secs: config.capture_delay_secs,
                        magnifier_enabled: config.magnifier_enabled,
                        save_location_setting: config.save_location,
                        custom_save_path: config.custom_save_path.clone(),
                        video_save_location_setting: config.video_save_location,
                        video_custom_save_path: config.video_custom_save_path.clone(),
                        toolbar_pos: HashMap::new(),
                        toolbar_dragging: false,
                        toolbar_drag_offset: None,
                        tesseract_available: is_tesseract_available(),
                        available_encoders: Vec::new(),
                        selected_encoder: config.video_encoder.clone(),
                        video_container: config.video_container,
                        video_framerate: config.video_framerate,
                        video_show_cursor: config.video_show_cursor,
                        show_cursor: config.show_cursor,
                        is_video_mode: false,
                        is_recording: false,
                        recording_annotation_mode: false,
                        pencil_popup_open: false,
                        pencil_color: config.pencil_color,
                        pencil_fade_duration: config.pencil_fade_duration,
                        pencil_thickness: config.pencil_thickness,
                        toolbar_bounds: None,
                        move_offset: None,
                        is_default_portal: is_default_portal(),
                    }
                },
            }))
            .await
        {
            // Distinct from the *response* path below: this means the UI event
            // loop isn't listening, so the request can't be shown at all.
            log::error!("Could not hand the screenshot request to the UI: {err}");
            return PortalResponse::Other;
        }
        rx.recv().await.unwrap_or(PortalResponse::Cancelled)
    }

    /// Not implemented. Declared so the frontend gets a portal response rather
    /// than an unknown-method error.
    // reason: zbus requires the method to be async and take `&self`.
    #[allow(clippy::unused_async, clippy::unused_self)]
    async fn pick_color(
        &self,
        _handle: zvariant::ObjectPath<'_>,
        _app_id: &str,
        _parent_window: &str,
        _options: HashMap<String, zvariant::OwnedValue>,
    ) -> PortalResponse<PickColorResult> {
        PortalResponse::Other
    }

    #[zbus(property, name = "version")]
    #[allow(clippy::unused_self)]
    const fn version(&self) -> u32 {
        2
    }
}

/// Whether `cosmic-portals.conf` names cosmic-x as the Screenshot portal.
pub fn is_default_portal() -> bool {
    let Some(config_dir) = dirs::config_dir() else {
        return false;
    };
    let conf_path = config_dir
        .join("xdg-desktop-portal")
        .join("cosmic-portals.conf");
    std::fs::read_to_string(&conf_path).is_ok_and(|contents| {
        contents
            .lines()
            .any(|l| l.trim() == "org.freedesktop.impl.portal.Screenshot=cosmic-x")
    })
}

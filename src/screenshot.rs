#![allow(dead_code, unused_variables)]

use cosmic::cosmic_config::CosmicConfigEntry;
use cosmic::iced::clipboard::mime::AsMimeTypes;
use cosmic::iced::keyboard::{Key, key::Named};
use cosmic::iced::{Limits, window};
use cosmic::iced_core::Length;
use cosmic::iced_runtime::clipboard;
use cosmic::iced_runtime::platform_specific::wayland::layer_surface::{
    IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced_winit::commands::layer_surface::{destroy_layer_surface, get_layer_surface};
use cosmic::widget::horizontal_space;
use cosmic_client_toolkit::sctk::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};
use futures::stream::{FuturesUnordered, StreamExt};
use image::RgbaImage;
use rustix::fd::AsFd;
use std::borrow::Cow;
use std::num::NonZeroU32;
use std::{collections::HashMap, io, path::PathBuf};
use tokio::sync::mpsc::Sender;

use wayland_client::protocol::wl_output::WlOutput;
use zbus::zvariant;

use crate::app::{App, OutputState};
use crate::config::{BlazingshotConfig, SaveLocation};
use crate::wayland::{CaptureSource, ShmImage, WaylandHelper};
use crate::widget::{keyboard_wrapper::KeyboardWrapper, rectangle_selection::DragState};
use crate::{PortalResponse, fl};

#[derive(Clone, Debug)]
pub struct ScreenshotImage {
    pub rgba: RgbaImage,
    pub handle: cosmic::widget::image::Handle,
}

impl ScreenshotImage {
    fn new<T: AsFd>(img: ShmImage<T>) -> anyhow::Result<Self> {
        let rgba = img.image_transformed()?;
        let handle = cosmic::widget::image::Handle::from_rgba(
            rgba.width(),
            rgba.height(),
            rgba.clone().into_vec(),
        );
        Ok(Self { rgba, handle })
    }

    pub fn width(&self) -> u32 {
        self.rgba.width()
    }

    pub fn height(&self) -> u32 {
        self.rgba.height()
    }
}

// Re-export OCR types from the ocr module
pub use crate::ocr::{OcrMapping, OcrStatus, OcrTextOverlay};

// Re-export QR types from the qr module
pub use crate::qr::DetectedQrCode;

// Re-export arrow/redact types from the arrow module
pub use crate::arrow::{ArrowAnnotation, RedactAnnotation};

// Arrow/redact functions are now in crate::arrow module
use crate::arrow::{draw_arrows_on_image, draw_redactions_on_image};

// OCR functions are now in crate::ocr module
use crate::ocr::{models_need_download, run_ocr_on_image_with_status};

// QR functions are now in crate::qr module
use crate::qr::{detect_qr_codes_at_resolution, is_duplicate_qr};

#[derive(zvariant::DeserializeDict, zvariant::Type, Clone, Debug)]
#[zvariant(signature = "a{sv}")]
pub struct ScreenshotOptions {
    modal: Option<bool>,
    interactive: Option<bool>,
    choose_destination: Option<bool>,
}

#[derive(zvariant::SerializeDict, zvariant::Type)]
#[zvariant(signature = "a{sv}")]
pub struct ScreenshotResult {
    uri: String,
}

struct ScreenshotBytes {
    bytes: Vec<u8>,
}

impl ScreenshotBytes {
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl AsMimeTypes for ScreenshotBytes {
    fn available(&self) -> std::borrow::Cow<'static, [String]> {
        Cow::Owned(vec!["image/png".to_string()])
    }

    fn as_bytes(&self, mime_type: &str) -> Option<std::borrow::Cow<'static, [u8]>> {
        Some(Cow::Owned(self.bytes.clone()))
    }
}

#[derive(zvariant::SerializeDict, zvariant::Type)]
#[zvariant(signature = "a{sv}")]
struct PickColorResult {
    color: (f64, f64, f64),
}

/// Logical Size and Position of a rectangle
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn intersect(&self, other: Rect) -> Option<Rect> {
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        let right = self.right.min(other.right);
        let bottom = self.bottom.min(other.bottom);
        if left < right && top < bottom {
            Some(Rect {
                left,
                top,
                right,
                bottom,
            })
        } else {
            None
        }
    }

    fn translate(&self, x: i32, y: i32) -> Rect {
        Rect {
            left: self.left + x,
            top: self.top + y,
            right: self.right + x,
            bottom: self.bottom + y,
        }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn dimensions(self) -> Option<RectDimension> {
        let width = NonZeroU32::new((self.width()).unsigned_abs())?;
        let height = NonZeroU32::new((self.height()).unsigned_abs())?;
        Some(RectDimension { width, height })
    }
}

#[derive(Clone, Copy)]
pub struct RectDimension {
    width: NonZeroU32,
    height: NonZeroU32,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSaveLocation {
    Clipboard,
    #[default]
    Pictures,
    Documents,
}

pub struct Screenshot {
    wayland_helper: WaylandHelper,
    tx: Sender<Event>,
}

impl Screenshot {
    pub fn new(wayland_helper: WaylandHelper, tx: Sender<Event>) -> Self {
        Self { wayland_helper, tx }
    }

    async fn interactive_toplevel_images(
        &self,
        outputs: &[Output],
    ) -> anyhow::Result<HashMap<String, Vec<ScreenshotImage>>> {
        let wayland_helper = self.wayland_helper.clone();
        Ok(outputs
            .iter()
            .map(move |Output { output, name, .. }| {
                let wayland_helper = wayland_helper.clone();
                async move {
                    let frame = wayland_helper
                        .capture_output_toplevels_shm(output, false)
                        .filter_map(|img| async { ScreenshotImage::new(img).ok() })
                        .collect()
                        .await;
                    (name.clone(), frame)
                }
            })
            .collect::<FuturesUnordered<_>>()
            .collect::<HashMap<String, _>>()
            .await)
    }

    async fn interactive_output_images(
        &self,
        outputs: &[Output],
        app_id: &str,
    ) -> anyhow::Result<HashMap<String, ScreenshotImage>> {
        let wayland_helper = self.wayland_helper.clone();

        let mut map = HashMap::with_capacity(outputs.len());
        for Output {
            output,
            logical_position: (output_x, output_y),
            name,
            ..
        } in outputs
        {
            let frame = wayland_helper
                .capture_source_shm(CaptureSource::Output(output.clone()), false)
                .await
                .ok_or_else(|| anyhow::anyhow!("shm screencopy failed"))?;
            map.insert(name.clone(), ScreenshotImage::new(frame)?);
        }

        Ok(map)
    }

    pub fn save_rgba(img: &RgbaImage, path: &PathBuf) -> anyhow::Result<()> {
        let mut file = std::fs::File::create(path)?;
        Ok(write_png(&mut file, img)?)
    }

    pub fn save_rgba_to_buffer(img: &RgbaImage, buffer: &mut Vec<u8>) -> anyhow::Result<()> {
        Ok(write_png(buffer, img)?)
    }

    pub fn get_img_path(location: ImageSaveLocation) -> Option<PathBuf> {
        let mut path = match location {
            ImageSaveLocation::Pictures => {
                dirs::picture_dir().or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
            }
            ImageSaveLocation::Documents => {
                dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
            }
            ImageSaveLocation::Clipboard => None,
        }?;
        let name = chrono::Local::now()
            .format("Screenshot_%Y-%m-%d_%H-%M-%S.png")
            .to_string();
        path.push(name);

        Some(path)
    }

    async fn screenshot_inner(&self, outputs: &[Output], app_id: &str) -> anyhow::Result<PathBuf> {
        let wayland_helper = self.wayland_helper.clone();

        let mut bounds_opt: Option<Rect> = None;
        let mut frames = Vec::with_capacity(outputs.len());
        for Output {
            output,
            logical_position: (output_x, output_y),
            logical_size: (output_w, output_h),
            ..
        } in outputs
        {
            let frame = wayland_helper
                .capture_source_shm(CaptureSource::Output(output.clone()), false)
                .await
                .ok_or_else(|| anyhow::anyhow!("shm screencopy failed"))?;
            let frame_image = frame.image_transformed()?;
            let rect = Rect {
                left: *output_x,
                top: *output_y,
                right: output_x.saturating_add(*output_w),
                bottom: output_y.saturating_add(*output_h),
            };
            bounds_opt = Some(match bounds_opt.take() {
                Some(bounds) => Rect {
                    left: bounds.left.min(rect.left),
                    top: bounds.top.min(rect.top),
                    right: bounds.right.max(rect.right),
                    bottom: bounds.bottom.max(rect.bottom),
                },
                None => rect,
            });
            frames.push((frame_image, rect));
        }

        let (file, path) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let image = combined_image(bounds_opt.unwrap_or_default(), frames);

            let mut file = tempfile::Builder::new()
                .prefix("screenshot-")
                .suffix(".png")
                .tempfile()?;
            {
                write_png(&mut file, &image)?;
            }
            Ok(file.keep()?)
        })
        .await??;

        Ok(path)
    }
}

fn combined_image(bounds: Rect, frames: Vec<(RgbaImage, Rect)>) -> RgbaImage {
    if frames.len() == 1 {
        let (frame_image, rect) = &frames[0];

        let width_scale = frame_image.width() as f64 / rect.width() as f64;
        let height_scale = frame_image.height() as f64 / rect.height() as f64;

        let width = (bounds.width() as f64 * width_scale).max(0.) as u32;
        let height = (bounds.height() as f64 * height_scale).max(0.) as u32;
        let x = ((bounds.left - rect.left) as f64 * width_scale).max(0.) as u32;
        let y = ((bounds.top - rect.top) as f64 * height_scale).max(0.) as u32;

        return image::imageops::crop_imm(frame_image, x, y, width, height).to_image();
    }

    let width = bounds
        .right
        .saturating_sub(bounds.left)
        .try_into()
        .unwrap_or_default();
    let height = bounds
        .bottom
        .saturating_sub(bounds.top)
        .try_into()
        .unwrap_or_default();
    let mut image = image::RgbaImage::new(width, height);
    for (mut frame_image, rect) in frames {
        let width = rect.width() as u32;
        let height = rect.height() as u32;
        if frame_image.dimensions() != (width, height) {
            frame_image = image::imageops::resize(
                &frame_image,
                width,
                height,
                image::imageops::FilterType::Lanczos3,
            );
        };
        let x = i64::from(rect.left) - i64::from(bounds.left);
        let y = i64::from(rect.top) - i64::from(bounds.top);
        image::imageops::overlay(&mut image, &frame_image, x, y);
    }
    image
}

fn write_png<W: io::Write>(w: W, image: &RgbaImage) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(w, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(image.as_raw())
}

/// Toolbar position on the screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarPosition {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Capture,
    Cancel,
    Choice(Choice),
    OutputChanged(WlOutput),
    WindowChosen(String, usize),
    Location(usize),
    QrCodesDetected(Vec<DetectedQrCode>),
    QrRequested,
    QrCopyAndClose,
    OcrRequested,
    OcrCopyAndClose,
    OcrStatus(OcrStatus),
    OcrStatusClear,
    ArrowModeToggle,                        // toggle arrow drawing mode
    ArrowStart(f32, f32),                   // start drawing arrow at position
    ArrowEnd(f32, f32),                     // finish arrow at position
    ArrowCancel,                            // cancel current arrow drawing
    RedactModeToggle,                       // toggle redact drawing mode
    RedactStart(f32, f32),                  // start drawing redact rectangle at position
    RedactEnd(f32, f32),                    // finish redact rectangle at position
    RedactCancel,                           // cancel current redact drawing
    ToolbarPositionChange(ToolbarPosition), // change toolbar position
    CopyToClipboard,                        // capture and copy to clipboard
    SaveToPictures,                         // capture and save to Pictures folder
    OpenUrl(String),                        // open URL in browser using xdg-open
    ToggleSettingsDrawer,                   // toggle settings drawer visibility
    ToggleMagnifier,                        // toggle magnifier on/off
    SetSaveLocationPictures,                // set save location to Pictures
    SetSaveLocationDocuments,               // set save location to Documents
    ToggleCopyOnSave,                       // toggle copy to clipboard on save
    SelectRegionMode,                       // switch to rectangle selection mode (R)
    SelectWindowMode,                       // switch to window selection mode (W)
    SelectScreenMode,                       // select current screen/output (S)
}

#[derive(Debug, Clone)]
pub enum Choice {
    Output(String),
    Rectangle(Rect, DragState),
    Window(String, Option<usize>),
}

#[derive(Debug, Clone, Default)]
pub enum Action {
    #[default]
    ReturnPath,
    SaveToClipboard,
    SaveToPictures,
    SaveToDocuments,
    ChooseFolder,
    Choice(Choice),
}

#[derive(Clone, Debug)]
pub struct Args {
    pub handle: zvariant::ObjectPath<'static>,
    pub app_id: String,
    pub parent_window: String,
    pub options: ScreenshotOptions,
    pub output_images: HashMap<String, ScreenshotImage>,
    pub toplevel_images: HashMap<String, Vec<ScreenshotImage>>,
    pub tx: Sender<PortalResponse<ScreenshotResult>>,
    pub choice: Choice,
    pub location: ImageSaveLocation,
    pub action: Action,
    pub qr_codes: Vec<DetectedQrCode>,
    pub qr_scanning: bool,
    pub ocr_status: OcrStatus,
    pub ocr_overlays: Vec<OcrTextOverlay>,
    /// OCR text result for copying (stored separately from status)
    pub ocr_text: Option<String>,
    /// Arrow annotations drawn on the screenshot
    pub arrows: Vec<ArrowAnnotation>,
    /// Whether arrow drawing mode is active
    pub arrow_mode: bool,
    /// Current arrow being drawn (start point set, waiting for end point)
    pub arrow_drawing: Option<(f32, f32)>,
    /// Redaction annotations drawn on the screenshot
    pub redactions: Vec<RedactAnnotation>,
    /// Whether redact drawing mode is active
    pub redact_mode: bool,
    /// Current redaction being drawn (start point set, waiting for end point)
    pub redact_drawing: Option<(f32, f32)>,
    /// Toolbar position on screen
    pub toolbar_position: ToolbarPosition,
    /// Whether settings drawer is open
    pub settings_drawer_open: bool,
    /// Whether magnifier is enabled (persisted setting)
    pub magnifier_enabled: bool,
    /// Save location setting (Pictures or Documents)
    pub save_location_setting: SaveLocation,
    /// Whether to also copy to clipboard when saving (persisted setting)
    pub copy_to_clipboard_on_save: bool,
    /// Whether to also copy to clipboard for the current save operation
    pub also_copy_to_clipboard: bool,
}

struct Output {
    output: WlOutput,
    logical_position: (i32, i32),
    logical_size: (i32, i32),
    name: String,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Event {
    Screenshot(Args),
    Init(Sender<Event>),
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl Screenshot {
    async fn screenshot(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        handle: zvariant::ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: ScreenshotOptions,
    ) -> PortalResponse<ScreenshotResult> {
        let mut outputs = Vec::new();
        for output in self.wayland_helper.outputs() {
            let Some(info) = self.wayland_helper.output_info(&output) else {
                log::warn!("Output {:?} has no info", output);
                continue;
            };
            let Some(name) = info.name.clone() else {
                log::warn!("Output {:?} has no name", output);
                continue;
            };
            let Some(logical_position) = info.logical_position else {
                log::warn!("Output {:?} has no position", output);
                continue;
            };
            let Some(logical_size) = info.logical_size else {
                log::warn!("Output {:?} has no size", output);
                continue;
            };
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
        };

        // Always interactive for blazingshot
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let first_output = &*outputs[0].name;
        let output_images = self
            .interactive_output_images(&outputs, app_id)
            .await
            .unwrap_or_default();
        let toplevel_images = self
            .interactive_toplevel_images(&outputs)
            .await
            .unwrap_or_default();

        let choice = Choice::Rectangle(Rect::default(), DragState::default());

        // Load persisted config for settings
        let config = BlazingshotConfig::load();

        // Send UI immediately with empty QR codes, detection happens async
        if let Err(err) = self
            .tx
            .send(Event::Screenshot(Args {
                handle: handle.to_owned(),
                app_id: app_id.to_string(),
                parent_window: parent_window.to_string(),
                action: if options.choose_destination.unwrap_or_default() {
                    Action::SaveToClipboard
                } else {
                    Action::ReturnPath
                },
                options,
                output_images,
                toplevel_images,
                tx,
                location: ImageSaveLocation::Pictures,
                choice,
                qr_codes: Vec::new(),
                qr_scanning: false,
                ocr_status: OcrStatus::Idle,
                ocr_overlays: Vec::new(),
                ocr_text: None,
                arrows: Vec::new(),
                arrow_mode: false,
                arrow_drawing: None,
                redactions: Vec::new(),
                redact_mode: false,
                redact_drawing: None,
                toolbar_position: ToolbarPosition::default(),
                settings_drawer_open: false,
                magnifier_enabled: config.magnifier_enabled,
                save_location_setting: config.save_location,
                copy_to_clipboard_on_save: config.copy_to_clipboard_on_save,
                also_copy_to_clipboard: false,
            }))
            .await
        {
            log::error!("Failed to send screenshot event, {}", err);
            return PortalResponse::Other;
        }
        if let Some(res) = rx.recv().await {
            res
        } else {
            PortalResponse::Cancelled::<ScreenshotResult>
        }
    }

    async fn pick_color(
        &self,
        handle: zvariant::ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        option: HashMap<String, zvariant::Value<'_>>,
    ) -> PortalResponse<PickColorResult> {
        // TODO: implement color picker
        PortalResponse::Other
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

pub(crate) fn view(app: &App, id: window::Id) -> cosmic::Element<'_, Msg> {
    let Some((i, output)) = app.outputs.iter().enumerate().find(|(i, o)| o.id == id) else {
        return horizontal_space().width(Length::Fixed(1.0)).into();
    };
    let Some(args) = app.screenshot_args.as_ref() else {
        return horizontal_space().width(Length::Fixed(1.0)).into();
    };

    let Some(img) = args.output_images.get(&output.name) else {
        return horizontal_space().width(Length::Fixed(1.0)).into();
    };
    let theme = app.core.system_theme().cosmic();
    let output_name = output.name.clone();
    KeyboardWrapper::new(
        crate::widget::screenshot::ScreenshotSelection::new(
            args.choice.clone(),
            img,
            Msg::CopyToClipboard,
            Msg::SaveToPictures,
            Msg::Cancel,
            Msg::OcrRequested,
            Msg::OcrCopyAndClose,
            Msg::QrRequested,
            Msg::QrCopyAndClose,
            output,
            id,
            Msg::OutputChanged,
            Msg::Choice,
            &args.toplevel_images,
            Msg::WindowChosen,
            theme.spacing,
            i as u128,
            &args.qr_codes,
            args.qr_scanning,
            &args.ocr_overlays,
            args.ocr_status.clone(),
            args.ocr_text.is_some(),
            &args.arrows,
            args.arrow_mode,
            args.arrow_drawing,
            Msg::ArrowModeToggle,
            Msg::ArrowStart,
            Msg::ArrowEnd,
            &args.redactions,
            args.redact_mode,
            args.redact_drawing,
            Msg::RedactModeToggle,
            Msg::RedactStart,
            Msg::RedactEnd,
            args.toolbar_position,
            Msg::ToolbarPositionChange,
            Msg::OpenUrl,
            args.settings_drawer_open,
            args.magnifier_enabled,
            Msg::ToggleSettingsDrawer,
            Msg::ToggleMagnifier,
            args.save_location_setting,
            Msg::SetSaveLocationPictures,
            Msg::SetSaveLocationDocuments,
            args.copy_to_clipboard_on_save,
            Msg::ToggleCopyOnSave,
            app.outputs.len(),
        ),
        {
            // Determine if we have a complete selection for action shortcuts
            let has_selection = match &args.choice {
                Choice::Rectangle(r, _) => r.dimensions().is_some(),
                Choice::Window(_, Some(_)) => true,
                Choice::Output(_) => true,
                _ => false,
            };
            let arrow_mode = args.arrow_mode;
            let redact_mode = args.redact_mode;

            move |key, modifiers| match key {
                // Save/copy shortcuts (always available - empty selection captures all screens)
                Key::Named(Named::Enter) if modifiers.control() => Some(Msg::SaveToPictures),
                Key::Named(Named::Enter) => Some(Msg::CopyToClipboard),
                Key::Named(Named::Escape) => Some(Msg::Cancel),
                // Mode toggle shortcuts (require selection)
                Key::Character(c) if c.as_str() == "a" && has_selection => {
                    Some(Msg::ArrowModeToggle)
                }
                Key::Character(c) if c.as_str() == "d" && has_selection => {
                    Some(Msg::RedactModeToggle)
                }
                // Selection mode shortcuts (always available, but not when in draw mode)
                Key::Character(c) if c.as_str() == "r" && !arrow_mode && !redact_mode => {
                    Some(Msg::SelectRegionMode)
                }
                Key::Character(c) if c.as_str() == "w" && !arrow_mode && !redact_mode => {
                    Some(Msg::SelectWindowMode)
                }
                Key::Character(c) if c.as_str() == "s" && !arrow_mode && !redact_mode => {
                    Some(Msg::SelectScreenMode)
                }
                _ => None,
            }
        },
    )
    .into()
}

pub fn update_msg(app: &mut App, msg: Msg) -> cosmic::Task<crate::app::Msg> {
    match msg {
        Msg::Capture => {
            let mut cmds: Vec<cosmic::Task<crate::app::Msg>> = app
                .outputs
                .iter()
                .map(|o| destroy_layer_surface(o.id))
                .collect();
            let Some(args) = app.screenshot_args.take() else {
                log::error!("Failed to find screenshot Args for Capture message.");
                return cosmic::Task::batch(cmds);
            };
            let outputs = app.outputs.clone();
            let Args {
                tx,
                choice,
                output_images: mut images,
                location,
                arrows,
                redactions,
                also_copy_to_clipboard,
                ..
            } = args;

            let mut success = true;
            let image_path = Screenshot::get_img_path(location);

            match choice {
                Choice::Output(output_name) => {
                    if let Some(img) = images.remove(&output_name) {
                        let mut final_img = img.rgba.clone();

                        // Draw arrows/redactions if any (they are in global coords, output_rect is also global)
                        if !arrows.is_empty() || !redactions.is_empty() {
                            // Find the output to get scale factor and position
                            if let Some(output) = outputs.iter().find(|o| o.name == output_name) {
                                let scale = final_img.width() as f32 / output.logical_size.0 as f32;

                                // Output rect in global coordinates
                                let output_rect = Rect {
                                    left: output.logical_pos.0,
                                    top: output.logical_pos.1,
                                    right: output.logical_pos.0 + output.logical_size.0 as i32,
                                    bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                                };
                                draw_redactions_on_image(
                                    &mut final_img,
                                    &redactions,
                                    &output_rect,
                                    scale,
                                );
                                draw_arrows_on_image(&mut final_img, &arrows, &output_rect, scale);
                            }
                        }

                        if let Some(ref image_path) = image_path {
                            if let Err(err) = Screenshot::save_rgba(&final_img, image_path) {
                                log::error!("Failed to capture screenshot: {:?}", err);
                            };
                            // Also copy to clipboard if enabled
                            if also_copy_to_clipboard {
                                let mut buffer = Vec::new();
                                if let Err(e) =
                                    Screenshot::save_rgba_to_buffer(&final_img, &mut buffer)
                                {
                                    log::error!("Failed to save screenshot to buffer: {:?}", e);
                                } else {
                                    cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                                }
                            }
                        } else {
                            let mut buffer = Vec::new();
                            if let Err(e) = Screenshot::save_rgba_to_buffer(&final_img, &mut buffer)
                            {
                                log::error!("Failed to save screenshot to buffer: {:?}", e);
                                success = false;
                            } else {
                                cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)))
                            };
                        }
                    } else {
                        log::error!("Failed to find output {}", output_name);
                        success = false;
                    }
                }
                Choice::Rectangle(r, s) => {
                    if let Some(RectDimension { width, height }) = r.dimensions() {
                        // Calculate the scale factor from the first intersecting output
                        // to determine target resolution
                        let target_scale = images
                            .iter()
                            .find_map(|(name, raw_img)| {
                                let output = outputs.iter().find(|o| o.name == *name)?;
                                let output_rect = Rect {
                                    left: output.logical_pos.0,
                                    top: output.logical_pos.1,
                                    right: output.logical_pos.0 + output.logical_size.0 as i32,
                                    bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                                };
                                r.intersect(output_rect)?;
                                Some(raw_img.rgba.width() as f32 / output.logical_size.0 as f32)
                            })
                            .unwrap_or(1.0);

                        // Scale selection rect to physical coordinates
                        let physical_bounds = Rect {
                            left: (r.left as f32 * target_scale) as i32,
                            top: (r.top as f32 * target_scale) as i32,
                            right: (r.right as f32 * target_scale) as i32,
                            bottom: (r.bottom as f32 * target_scale) as i32,
                        };

                        let frames = images
                            .into_iter()
                            .filter_map(|(name, raw_img)| {
                                let output = outputs.iter().find(|o| o.name == name)?;
                                let pos = output.logical_pos;
                                let output_rect = Rect {
                                    left: pos.0,
                                    top: pos.1,
                                    right: pos.0 + output.logical_size.0 as i32,
                                    bottom: pos.1 + output.logical_size.1 as i32,
                                };

                                let intersect = r.intersect(output_rect)?;

                                // Crop to intersection in physical coordinates
                                let scale_x =
                                    raw_img.rgba.width() as f32 / output.logical_size.0 as f32;
                                let scale_y =
                                    raw_img.rgba.height() as f32 / output.logical_size.1 as f32;

                                let img_x =
                                    ((intersect.left - output_rect.left) as f32 * scale_x) as u32;
                                let img_y =
                                    ((intersect.top - output_rect.top) as f32 * scale_y) as u32;
                                let img_w = (intersect.width() as f32 * scale_x) as u32;
                                let img_h = (intersect.height() as f32 * scale_y) as u32;

                                let cropped = image::imageops::crop_imm(
                                    &raw_img.rgba,
                                    img_x,
                                    img_y,
                                    img_w,
                                    img_h,
                                )
                                .to_image();

                                // Physical rect for this cropped portion
                                let physical_intersect = Rect {
                                    left: (intersect.left as f32 * target_scale) as i32,
                                    top: (intersect.top as f32 * target_scale) as i32,
                                    right: (intersect.right as f32 * target_scale) as i32,
                                    bottom: (intersect.bottom as f32 * target_scale) as i32,
                                };

                                Some((cropped, physical_intersect))
                            })
                            .collect::<Vec<_>>();
                        let mut img = combined_image(physical_bounds, frames);

                        // Draw redactions and arrows onto the final image
                        if !redactions.is_empty() {
                            draw_redactions_on_image(&mut img, &redactions, &r, target_scale);
                        }
                        if !arrows.is_empty() {
                            draw_arrows_on_image(&mut img, &arrows, &r, target_scale);
                        }

                        if let Some(ref image_path) = image_path {
                            if let Err(err) = Screenshot::save_rgba(&img, image_path) {
                                success = false;
                            }
                            // Also copy to clipboard if enabled
                            if also_copy_to_clipboard {
                                let mut buffer = Vec::new();
                                if let Err(e) = Screenshot::save_rgba_to_buffer(&img, &mut buffer) {
                                    log::error!("Failed to save screenshot to buffer: {:?}", e);
                                } else {
                                    cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                                }
                            }
                        } else {
                            let mut buffer = Vec::new();
                            if let Err(e) = Screenshot::save_rgba_to_buffer(&img, &mut buffer) {
                                log::error!("Failed to save screenshot to buffer: {:?}", e);
                                success = false;
                            } else {
                                cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)))
                            };
                        }
                    } else {
                        // Empty selection - capture all screens combined
                        // Calculate bounds that encompass all outputs
                        let mut all_bounds: Option<Rect> = None;
                        for output in &outputs {
                            let output_rect = Rect {
                                left: output.logical_pos.0,
                                top: output.logical_pos.1,
                                right: output.logical_pos.0 + output.logical_size.0 as i32,
                                bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                            };
                            all_bounds = Some(match all_bounds.take() {
                                Some(bounds) => Rect {
                                    left: bounds.left.min(output_rect.left),
                                    top: bounds.top.min(output_rect.top),
                                    right: bounds.right.max(output_rect.right),
                                    bottom: bounds.bottom.max(output_rect.bottom),
                                },
                                None => output_rect,
                            });
                        }

                        if let Some(logical_bounds) = all_bounds {
                            // Get scale from first output
                            let target_scale = images
                                .values()
                                .next()
                                .and_then(|img| {
                                    outputs
                                        .first()
                                        .map(|o| img.rgba.width() as f32 / o.logical_size.0 as f32)
                                })
                                .unwrap_or(1.0);

                            let physical_bounds = Rect {
                                left: (logical_bounds.left as f32 * target_scale) as i32,
                                top: (logical_bounds.top as f32 * target_scale) as i32,
                                right: (logical_bounds.right as f32 * target_scale) as i32,
                                bottom: (logical_bounds.bottom as f32 * target_scale) as i32,
                            };

                            let frames = images
                                .into_iter()
                                .filter_map(|(name, raw_img)| {
                                    let output = outputs.iter().find(|o| o.name == name)?;
                                    let pos = output.logical_pos;

                                    // Physical rect for this output
                                    let physical_rect = Rect {
                                        left: (pos.0 as f32 * target_scale) as i32,
                                        top: (pos.1 as f32 * target_scale) as i32,
                                        right: ((pos.0 + output.logical_size.0 as i32) as f32
                                            * target_scale)
                                            as i32,
                                        bottom: ((pos.1 + output.logical_size.1 as i32) as f32
                                            * target_scale)
                                            as i32,
                                    };

                                    Some((raw_img.rgba, physical_rect))
                                })
                                .collect::<Vec<_>>();

                            let img = combined_image(physical_bounds, frames);

                            if let Some(ref image_path) = image_path {
                                if let Err(err) = Screenshot::save_rgba(&img, image_path) {
                                    log::error!("Failed to capture screenshot: {:?}", err);
                                    success = false;
                                }
                                // Also copy to clipboard if enabled
                                if also_copy_to_clipboard {
                                    let mut buffer = Vec::new();
                                    if let Err(e) =
                                        Screenshot::save_rgba_to_buffer(&img, &mut buffer)
                                    {
                                        log::error!("Failed to save screenshot to buffer: {:?}", e);
                                    } else {
                                        cmds.push(clipboard::write_data(ScreenshotBytes::new(
                                            buffer,
                                        )));
                                    }
                                }
                            } else {
                                let mut buffer = Vec::new();
                                if let Err(e) = Screenshot::save_rgba_to_buffer(&img, &mut buffer) {
                                    log::error!("Failed to save screenshot to buffer: {:?}", e);
                                    success = false;
                                } else {
                                    cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)))
                                };
                            }
                        } else {
                            log::error!("No outputs available for all-screens capture");
                            success = false;
                        }
                    }
                }
                Choice::Window(output_name, Some(window_i)) => {
                    if let Some(img) = args
                        .toplevel_images
                        .get(&output_name)
                        .and_then(|imgs| imgs.get(window_i))
                    {
                        let mut final_img = img.rgba.clone();

                        // Draw arrows/redactions if any
                        // They are stored in output-relative coords where the window was displayed centered
                        if !arrows.is_empty() || !redactions.is_empty() {
                            // Find the output to calculate where the window was displayed
                            if let Some(output) = outputs.iter().find(|o| o.name == output_name) {
                                let img_width = final_img.width() as f32;
                                let img_height = final_img.height() as f32;
                                let output_width = output.logical_size.0 as f32;
                                let output_height = output.logical_size.1 as f32;

                                // Match the centering logic in SelectedImageWidget::image_bounds (20px margin)
                                let available_width = output_width - 20.0;
                                let available_height = output_height - 20.0;
                                let scale_x = available_width / img_width;
                                let scale_y = available_height / img_height;
                                let display_scale = scale_x.min(scale_y).min(1.0);

                                let display_width = img_width * display_scale;
                                let display_height = img_height * display_scale;
                                let sel_x = (output_width - display_width) / 2.0;
                                let sel_y = (output_height - display_height) / 2.0;

                                // The selection_rect is where the window was displayed on screen (in global coords)
                                // Arrow/redact coords are stored in global coordinates (output.left + pos.x)
                                // Image scale factor is 1/display_scale (to go from display to original)
                                let output_left = output.logical_pos.0 as f32;
                                let output_top = output.logical_pos.1 as f32;
                                let window_rect = Rect {
                                    left: (output_left + sel_x) as i32,
                                    top: (output_top + sel_y) as i32,
                                    right: (output_left + sel_x + display_width) as i32,
                                    bottom: (output_top + sel_y + display_height) as i32,
                                };
                                let image_scale = 1.0 / display_scale;
                                draw_redactions_on_image(
                                    &mut final_img,
                                    &redactions,
                                    &window_rect,
                                    image_scale,
                                );
                                draw_arrows_on_image(
                                    &mut final_img,
                                    &arrows,
                                    &window_rect,
                                    image_scale,
                                );
                            }
                        }

                        if let Some(ref image_path) = image_path {
                            if let Err(err) = Screenshot::save_rgba(&final_img, image_path) {
                                log::error!("Failed to capture screenshot: {:?}", err);
                                success = false;
                            }
                            // Also copy to clipboard if enabled
                            if also_copy_to_clipboard {
                                let mut buffer = Vec::new();
                                if let Err(e) =
                                    Screenshot::save_rgba_to_buffer(&final_img, &mut buffer)
                                {
                                    log::error!("Failed to save screenshot to buffer: {:?}", e);
                                } else {
                                    cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                                }
                            }
                        } else {
                            let mut buffer = Vec::new();
                            if let Err(e) = Screenshot::save_rgba_to_buffer(&final_img, &mut buffer)
                            {
                                log::error!("Failed to save screenshot to buffer: {:?}", e);
                                success = false;
                            } else {
                                cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)))
                            };
                        }
                    } else {
                        success = false;
                    }
                }
                _ => {
                    success = false;
                }
            }

            let response = if success && let Some(image_path1) = image_path {
                PortalResponse::Success(ScreenshotResult {
                    uri: format!("file:///{}", image_path1.display()),
                })
            } else if success && image_path.is_none() {
                PortalResponse::Success(ScreenshotResult {
                    uri: "clipboard:///".to_string(),
                })
            } else {
                PortalResponse::Other
            };

            tokio::spawn(async move {
                if let Err(err) = tx.send(response).await {
                    log::error!("Failed to send screenshot event");
                }
            });
            cosmic::Task::batch(cmds)
        }
        Msg::Cancel => {
            let cmds = app.outputs.iter().map(|o| destroy_layer_surface(o.id));
            let Some(args) = app.screenshot_args.take() else {
                log::error!("Failed to find screenshot Args for Cancel message.");
                return cosmic::Task::batch(cmds);
            };
            let Args { tx, .. } = args;
            tokio::spawn(async move {
                if let Err(err) = tx.send(PortalResponse::Cancelled).await {
                    log::error!("Failed to send screenshot event");
                }
            });

            cosmic::Task::batch(cmds)
        }
        Msg::Choice(c) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Clear OCR/QR/arrows when rectangle changes (new selection started)
                if let Choice::Rectangle(new_r, new_s) = &c {
                    if let Choice::Rectangle(old_r, _) = &args.choice {
                        // If the rectangle position/size changed significantly, clear everything
                        if new_r.left != old_r.left
                            || new_r.top != old_r.top
                            || new_r.right != old_r.right
                            || new_r.bottom != old_r.bottom
                        {
                            args.ocr_overlays.clear();
                            args.ocr_status = OcrStatus::Idle;
                            args.ocr_text = None;
                            args.qr_codes.clear();
                            args.arrows.clear();
                            args.arrow_mode = false;
                            args.arrow_drawing = None;
                            args.redactions.clear();
                            args.redact_mode = false;
                            args.redact_drawing = None;
                        }
                    }
                    // Also clear if we're starting a new drag from None state
                    if *new_s != DragState::None {
                        args.ocr_overlays.clear();
                        args.ocr_status = OcrStatus::Idle;
                        args.ocr_text = None;
                        args.qr_codes.clear();
                        args.arrows.clear();
                        args.arrow_mode = false;
                        args.arrow_drawing = None;
                        args.redactions.clear();
                        args.redact_mode = false;
                        args.redact_drawing = None;
                    }
                }
                // Clear arrows/redactions when switching modes (Region, Window, or Output)
                if matches!(
                    &c,
                    Choice::Rectangle(_, DragState::None)
                        | Choice::Window(_, None)
                        | Choice::Output(_)
                ) {
                    args.arrows.clear();
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    args.redactions.clear();
                    args.redact_mode = false;
                    args.redact_drawing = None;
                }
                args.choice = c;
                if let Choice::Rectangle(r, s) = &args.choice {
                    app.prev_rectangle = Some(*r);
                }
            } else {
                log::error!("Failed to find screenshot Args for Choice message.");
            }
            cosmic::Task::none()
        }
        Msg::OutputChanged(wl_output) => {
            if let (Some(args), Some(o)) = (
                app.screenshot_args.as_mut(),
                app.outputs
                    .iter()
                    .find(|o| o.output == wl_output)
                    .map(|o| o.name.clone()),
            ) {
                args.choice = Choice::Output(o);
                // Clear arrows/redactions when selecting an output
                args.arrows.clear();
                args.arrow_mode = false;
                args.arrow_drawing = None;
                args.redactions.clear();
                args.redact_mode = false;
                args.redact_drawing = None;
            } else {
                log::error!(
                    "Failed to find output for OutputChange message: {:?}",
                    wl_output
                );
            }
            app.active_output = Some(wl_output);
            cosmic::Task::none()
        }
        Msg::WindowChosen(name, i) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.choice = Choice::Window(name, Some(i));
                // Clear any previous OCR/QR state when selecting a new window
                args.ocr_status = OcrStatus::Idle;
                args.ocr_overlays.clear();
                args.ocr_text = None;
                args.qr_codes.clear();
                args.qr_scanning = false;
                args.arrows.clear();
                args.arrow_mode = false;
                args.arrow_drawing = None;
                args.redactions.clear();
                args.redact_mode = false;
                args.redact_drawing = None;
            } else {
                log::error!("Failed to find screenshot Args for WindowChosen message.");
            }
            // Don't capture immediately - let user interact with OCR/QR/arrow buttons
            cosmic::Task::none()
        }
        Msg::Location(loc) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                let loc = match loc {
                    loc if loc == ImageSaveLocation::Clipboard as usize => {
                        ImageSaveLocation::Clipboard
                    }
                    loc if loc == ImageSaveLocation::Pictures as usize => {
                        ImageSaveLocation::Pictures
                    }
                    loc if loc == ImageSaveLocation::Documents as usize => {
                        ImageSaveLocation::Documents
                    }
                    _ => args.location,
                };
                args.location = loc;
                cosmic::Task::none()
            } else {
                log::error!("Failed to find screenshot Args for Location message.");
                cosmic::Task::none()
            }
        }
        Msg::QrRequested => {
            // Clear previous QR codes and OCR overlays, start scanning
            if let Some(args) = app.screenshot_args.as_mut() {
                args.qr_codes.clear();
                args.qr_scanning = true;
                // Hide OCR overlays when running QR
                args.ocr_overlays.clear();
                args.ocr_status = OcrStatus::Idle;
                args.ocr_text = None;
                // Clear arrows when running QR (keep redactions)
                args.arrows.clear();
                args.arrow_mode = false;
                args.arrow_drawing = None;
                // Toggle off redact mode (keep redactions themselves)
                args.redact_mode = false;
                args.redact_drawing = None;
            }

            // Get the selection and run QR detection on that area
            if let Some(args) = app.screenshot_args.as_ref() {
                let redactions = args.redactions.clone();
                let outputs_clone = app.outputs.clone();

                // Get image data and parameters based on choice type
                // Returns: (image, output_name, scale, origin_x, origin_y, selection_rect_for_redactions)
                let qr_params: Option<(RgbaImage, String, f32, f32, f32, Rect)> = match &args.choice
                {
                    Choice::Rectangle(rect, _) if rect.width() > 0 && rect.height() > 0 => {
                        let mut params = None;
                        for output in &app.outputs {
                            if let Some(img) = args.output_images.get(&output.name) {
                                let output_rect = Rect {
                                    left: output.logical_pos.0,
                                    top: output.logical_pos.1,
                                    right: output.logical_pos.0 + output.logical_size.0 as i32,
                                    bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                                };

                                if let Some(intersection) = rect.intersect(output_rect) {
                                    let scale =
                                        img.rgba.width() as f32 / output.logical_size.0 as f32;
                                    let x = ((intersection.left - output_rect.left) as f32 * scale)
                                        as u32;
                                    let y = ((intersection.top - output_rect.top) as f32 * scale)
                                        as u32;
                                    let w = (intersection.width() as f32 * scale) as u32;
                                    let h = (intersection.height() as f32 * scale) as u32;

                                    let cropped =
                                        image::imageops::crop_imm(&img.rgba, x, y, w, h).to_image();
                                    let origin_x = (intersection.left - output_rect.left) as f32;
                                    let origin_y = (intersection.top - output_rect.top) as f32;

                                    // Selection rect is the intersection in global coords
                                    params = Some((
                                        cropped,
                                        output.name.clone(),
                                        scale,
                                        origin_x,
                                        origin_y,
                                        intersection,
                                    ));
                                    break;
                                }
                            }
                        }
                        params
                    }
                    Choice::Window(output_name, Some(window_index)) => {
                        args.toplevel_images
                            .get(output_name)
                            .and_then(|imgs| imgs.get(*window_index))
                            .and_then(|img| {
                                // Calculate where the window was displayed (matching Capture logic)
                                outputs_clone.iter().find(|o| &o.name == output_name).map(
                                    |output| {
                                        let img_width = img.rgba.width() as f32;
                                        let img_height = img.rgba.height() as f32;
                                        let output_width = output.logical_size.0 as f32;
                                        let output_height = output.logical_size.1 as f32;

                                        let available_width = output_width - 20.0;
                                        let available_height = output_height - 20.0;
                                        let scale_x = available_width / img_width;
                                        let scale_y = available_height / img_height;
                                        let display_scale = scale_x.min(scale_y).min(1.0);

                                        let display_width = img_width * display_scale;
                                        let display_height = img_height * display_scale;
                                        let sel_x = (output_width - display_width) / 2.0;
                                        let sel_y = (output_height - display_height) / 2.0;

                                        let output_left = output.logical_pos.0 as f32;
                                        let output_top = output.logical_pos.1 as f32;
                                        let window_rect = Rect {
                                            left: (output_left + sel_x) as i32,
                                            top: (output_top + sel_y) as i32,
                                            right: (output_left + sel_x + display_width) as i32,
                                            bottom: (output_top + sel_y + display_height) as i32,
                                        };
                                        let img_scale = 1.0 / display_scale;

                                        (
                                            img.rgba.clone(),
                                            output_name.clone(),
                                            img_scale,
                                            0.0,
                                            0.0,
                                            window_rect,
                                        )
                                    },
                                )
                            })
                    }
                    Choice::Output(output_name) => {
                        args.output_images.get(output_name).and_then(|img| {
                            outputs_clone
                                .iter()
                                .find(|o| &o.name == output_name)
                                .map(|output| {
                                    let scale =
                                        img.rgba.width() as f32 / output.logical_size.0 as f32;
                                    let output_rect = Rect {
                                        left: output.logical_pos.0,
                                        top: output.logical_pos.1,
                                        right: output.logical_pos.0 + output.logical_size.0 as i32,
                                        bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                                    };
                                    (
                                        img.rgba.clone(),
                                        output_name.clone(),
                                        scale,
                                        0.0,
                                        0.0,
                                        output_rect,
                                    )
                                })
                        })
                    }
                    _ => None,
                };

                if let Some((mut cropped, output_name, scale, origin_x, origin_y, selection_rect)) =
                    qr_params
                {
                    // Apply redactions to the image before QR scanning
                    if !redactions.is_empty() {
                        draw_redactions_on_image(&mut cropped, &redactions, &selection_rect, scale);
                    }
                    // Spawn progressive QR detection tasks (3 passes with increasing resolution)
                    let resolutions = [500u32, 1500, 0]; // 0 = full resolution
                    let mut qr_detection_tasks = Vec::new();

                    resolutions.into_iter().for_each(|max_dim| {
                        let cropped_clone = cropped.clone();
                        let output_name_clone = output_name.clone();
                        let task = cosmic::Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    let detected = detect_qr_codes_at_resolution(
                                        &cropped_clone,
                                        &output_name_clone,
                                        scale,
                                        max_dim,
                                    );
                                    detected
                                        .into_iter()
                                        .map(|mut qr| {
                                            qr.center_x += origin_x;
                                            qr.center_y += origin_y;
                                            qr
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .await
                                .unwrap_or_default()
                            },
                            move |qr_codes| {
                                crate::app::Msg::Screenshot(Msg::QrCodesDetected(qr_codes))
                            },
                        );
                        qr_detection_tasks.push(task);
                    });

                    return cosmic::Task::batch(qr_detection_tasks);
                }
            }
            cosmic::Task::none()
        }
        Msg::QrCodesDetected(new_qr_codes) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Scanning pass completed - hide scanning indicator after first pass
                args.qr_scanning = false;

                // Merge new QR codes, avoiding duplicates
                for qr in new_qr_codes {
                    if !is_duplicate_qr(&args.qr_codes, &qr) {
                        args.qr_codes.push(qr);
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::OcrRequested => {
            // Check if models need downloading and set appropriate status
            let needs_download = models_need_download();
            if let Some(args) = app.screenshot_args.as_mut() {
                args.ocr_status = if needs_download {
                    OcrStatus::DownloadingModels
                } else {
                    OcrStatus::Running
                };
                // Clear previous OCR overlays when starting a new run
                args.ocr_overlays.clear();
                args.ocr_text = None;
                // Hide QR codes when running OCR
                args.qr_codes.clear();
                // Clear arrows when running OCR (keep redactions)
                args.arrows.clear();
                args.arrow_mode = false;
                args.arrow_drawing = None;
                // Toggle off redact mode (keep redactions themselves)
                args.redact_mode = false;
                args.redact_drawing = None;
            }

            // Get the selection and run OCR on that area
            if let Some(args) = app.screenshot_args.as_ref() {
                let redactions = args.redactions.clone();
                let outputs_clone = app.outputs.clone();

                // Returns: (image, mapping, selection_rect_for_redactions, scale_for_redactions)
                let region_data: Option<(RgbaImage, OcrMapping, Rect, f32)> = match &args.choice {
                    Choice::Rectangle(rect, _) if rect.width() > 0 && rect.height() > 0 => {
                        // Collect image data for the selected rectangle
                        let mut data = None;
                        for output in &app.outputs {
                            if let Some(img) = args.output_images.get(&output.name) {
                                let output_rect = Rect {
                                    left: output.logical_pos.0,
                                    top: output.logical_pos.1,
                                    right: output.logical_pos.0 + output.logical_size.0 as i32,
                                    bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                                };

                                if let Some(intersection) = rect.intersect(output_rect) {
                                    let scale =
                                        img.rgba.width() as f32 / output.logical_size.0 as f32;
                                    let x = ((intersection.left - output_rect.left) as f32 * scale)
                                        as u32;
                                    let y = ((intersection.top - output_rect.top) as f32 * scale)
                                        as u32;
                                    let w = (intersection.width() as f32 * scale) as u32;
                                    let h = (intersection.height() as f32 * scale) as u32;

                                    let cropped =
                                        image::imageops::crop_imm(&img.rgba, x, y, w, h).to_image();

                                    let origin_x = (intersection.left - output_rect.left) as f32;
                                    let origin_y = (intersection.top - output_rect.top) as f32;
                                    let size_w = intersection.width() as f32;
                                    let size_h = intersection.height() as f32;

                                    data = Some((
                                        cropped,
                                        OcrMapping {
                                            origin: (origin_x, origin_y),
                                            size: (size_w, size_h),
                                            scale,
                                            output_name: output.name.clone(),
                                        },
                                        intersection,
                                        scale,
                                    ));
                                    break;
                                }
                            }
                        }
                        data
                    }
                    Choice::Window(output_name, Some(window_index)) => {
                        // Get window image from toplevel_images
                        args.toplevel_images
                            .get(output_name)
                            .and_then(|imgs| imgs.get(*window_index))
                            .and_then(|img| {
                                // Calculate where the window was displayed (matching Capture logic)
                                outputs_clone.iter().find(|o| &o.name == output_name).map(
                                    |output| {
                                        let img_width = img.rgba.width() as f32;
                                        let img_height = img.rgba.height() as f32;
                                        let output_width = output.logical_size.0 as f32;
                                        let output_height = output.logical_size.1 as f32;

                                        let available_width = output_width - 20.0;
                                        let available_height = output_height - 20.0;
                                        let scale_x = available_width / img_width;
                                        let scale_y = available_height / img_height;
                                        let display_scale = scale_x.min(scale_y).min(1.0);

                                        let display_width = img_width * display_scale;
                                        let display_height = img_height * display_scale;
                                        let sel_x = (output_width - display_width) / 2.0;
                                        let sel_y = (output_height - display_height) / 2.0;

                                        let output_left = output.logical_pos.0 as f32;
                                        let output_top = output.logical_pos.1 as f32;
                                        let window_rect = Rect {
                                            left: (output_left + sel_x) as i32,
                                            top: (output_top + sel_y) as i32,
                                            right: (output_left + sel_x + display_width) as i32,
                                            bottom: (output_top + sel_y + display_height) as i32,
                                        };
                                        let img_scale = 1.0 / display_scale;

                                        // OCR origin is where the window is displayed on the output (in output-relative coords)
                                        // OCR scale is display_scale (pixels per logical unit in the displayed image)
                                        // The image is the original, so OCR sees original pixels, but mapping needs display coords
                                        let ocr_scale = img.rgba.width() as f32 / display_width;

                                        (
                                            img.rgba.clone(),
                                            OcrMapping {
                                                origin: (sel_x, sel_y),
                                                size: (display_width, display_height),
                                                scale: ocr_scale,
                                                output_name: output_name.clone(),
                                            },
                                            window_rect,
                                            img_scale,
                                        )
                                    },
                                )
                            })
                    }
                    Choice::Output(output_name) => {
                        // Get full output image
                        args.output_images.get(output_name).and_then(|img| {
                            outputs_clone
                                .iter()
                                .find(|o| &o.name == output_name)
                                .map(|output| {
                                    let scale =
                                        img.rgba.width() as f32 / output.logical_size.0 as f32;
                                    let output_rect = Rect {
                                        left: output.logical_pos.0,
                                        top: output.logical_pos.1,
                                        right: output.logical_pos.0 + output.logical_size.0 as i32,
                                        bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                                    };
                                    (
                                        img.rgba.clone(),
                                        OcrMapping {
                                            origin: (0.0, 0.0),
                                            size: (
                                                img.rgba.width() as f32,
                                                img.rgba.height() as f32,
                                            ),
                                            scale: 1.0,
                                            output_name: output_name.clone(),
                                        },
                                        output_rect,
                                        scale,
                                    )
                                })
                        })
                    }
                    _ => None,
                };

                if let Some((mut cropped_img, mapping, selection_rect, scale)) = region_data {
                    // Apply redactions to the image before OCR
                    if !redactions.is_empty() {
                        draw_redactions_on_image(
                            &mut cropped_img,
                            &redactions,
                            &selection_rect,
                            scale,
                        );
                    }

                    // Run OCR in background with status updates
                    return cosmic::Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                run_ocr_on_image_with_status(&cropped_img, mapping)
                            })
                            .await
                            .unwrap_or_else(|_| OcrStatus::Error("OCR task panicked".to_string()))
                        },
                        |status| crate::app::Msg::Screenshot(Msg::OcrStatus(status)),
                    );
                }
            }
            cosmic::Task::none()
        }
        Msg::OcrStatus(status) => {
            match &status {
                OcrStatus::Done(text, overlays) => {
                    log::info!("OCR Result: {} ({} overlays)", text, overlays.len());
                    for overlay in overlays.iter() {
                        log::info!(
                            "  Overlay block {}: ({}, {}, {}x{}) on {}",
                            overlay.block_num,
                            overlay.left,
                            overlay.top,
                            overlay.width,
                            overlay.height,
                            overlay.output_name
                        );
                    }
                    if let Some(args) = app.screenshot_args.as_mut() {
                        args.ocr_status = status.clone();
                        args.ocr_overlays = overlays.clone();
                        // Store text for later copying when user clicks the button
                        if !text.is_empty() && text != "No text detected" {
                            args.ocr_text = Some(text.clone());
                        }
                        log::info!("Stored {} overlays in args", args.ocr_overlays.len());
                    }
                    // Don't auto-copy - user will click "copy text" button
                }
                OcrStatus::Error(err) => {
                    log::error!("OCR Error: {}", err);
                    if let Some(args) = app.screenshot_args.as_mut() {
                        args.ocr_status = status;
                        args.ocr_overlays.clear();
                        args.ocr_text = None;
                    }
                }
                _ => {
                    if let Some(args) = app.screenshot_args.as_mut() {
                        args.ocr_status = status;
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::OcrStatusClear => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.ocr_status = OcrStatus::Idle;
            }
            cosmic::Task::none()
        }
        Msg::OcrCopyAndClose => {
            // Copy OCR text and close the app
            let mut cmds: Vec<cosmic::Task<crate::app::Msg>> = app
                .outputs
                .iter()
                .map(|o| destroy_layer_surface(o.id))
                .collect();

            if let Some(args) = app.screenshot_args.take() {
                let Args { tx, ocr_text, .. } = args;

                if let Some(text) = ocr_text {
                    cmds.push(clipboard::write(text));
                }

                tokio::spawn(async move {
                    if let Err(err) = tx.send(PortalResponse::Cancelled).await {
                        log::error!("Failed to send screenshot event");
                    }
                });
            }
            cosmic::Task::batch(cmds)
        }
        Msg::QrCopyAndClose => {
            // Copy first QR code content and close the app
            let mut cmds: Vec<cosmic::Task<crate::app::Msg>> = app
                .outputs
                .iter()
                .map(|o| destroy_layer_surface(o.id))
                .collect();

            if let Some(args) = app.screenshot_args.take() {
                let Args { tx, qr_codes, .. } = args;

                // Copy first QR code content
                if let Some(qr) = qr_codes.first() {
                    cmds.push(clipboard::write(qr.content.clone()));
                }

                tokio::spawn(async move {
                    if let Err(err) = tx.send(PortalResponse::Cancelled).await {
                        log::error!("Failed to send screenshot event");
                    }
                });
            }
            cosmic::Task::batch(cmds)
        }
        Msg::ArrowModeToggle => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.arrow_mode = !args.arrow_mode;
                // Cancel any in-progress arrow when toggling off
                if !args.arrow_mode {
                    args.arrow_drawing = None;
                } else {
                    // Disable redact mode when enabling arrow mode (mutually exclusive)
                    args.redact_mode = false;
                    args.redact_drawing = None;
                    // Clear OCR/QR when enabling arrow mode
                    args.ocr_overlays.clear();
                    args.ocr_status = OcrStatus::Idle;
                    args.ocr_text = None;
                    args.qr_codes.clear();
                }
            }
            cosmic::Task::none()
        }
        Msg::ArrowStart(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && args.arrow_mode
            {
                args.arrow_drawing = Some((x, y));
            }
            cosmic::Task::none()
        }
        Msg::ArrowEnd(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && let Some((start_x, start_y)) = args.arrow_drawing.take()
            {
                args.arrows.push(ArrowAnnotation {
                    start_x,
                    start_y,
                    end_x: x,
                    end_y: y,
                });
            }
            cosmic::Task::none()
        }
        Msg::ArrowCancel => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.arrow_drawing = None;
            }
            cosmic::Task::none()
        }
        Msg::RedactModeToggle => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.redact_mode = !args.redact_mode;
                // Cancel any in-progress redaction when toggling off
                if !args.redact_mode {
                    args.redact_drawing = None;
                } else {
                    // Disable arrow mode when enabling redact mode (mutually exclusive)
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    // Clear OCR/QR when enabling redact mode (same as arrow)
                    args.ocr_overlays.clear();
                    args.ocr_status = OcrStatus::Idle;
                    args.ocr_text = None;
                    args.qr_codes.clear();
                }
            }
            cosmic::Task::none()
        }
        Msg::RedactStart(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && args.redact_mode
            {
                args.redact_drawing = Some((x, y));
            }
            cosmic::Task::none()
        }
        Msg::RedactEnd(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && let Some((start_x, start_y)) = args.redact_drawing.take()
            {
                args.redactions.push(RedactAnnotation {
                    x: start_x,
                    y: start_y,
                    x2: x,
                    y2: y,
                });
            }
            cosmic::Task::none()
        }
        Msg::RedactCancel => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.redact_drawing = None;
            }
            cosmic::Task::none()
        }
        Msg::ToolbarPositionChange(position) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.toolbar_position = position;
            }
            cosmic::Task::none()
        }
        Msg::CopyToClipboard => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.location = ImageSaveLocation::Clipboard;
            }
            update_msg(app, Msg::Capture)
        }
        Msg::SaveToPictures => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Use save location from settings
                args.location = match args.save_location_setting {
                    SaveLocation::Pictures => ImageSaveLocation::Pictures,
                    SaveLocation::Documents => ImageSaveLocation::Documents,
                };
                // Check if we should also copy to clipboard
                if args.copy_to_clipboard_on_save {
                    args.also_copy_to_clipboard = true;
                }
            }
            update_msg(app, Msg::Capture)
        }
        Msg::OpenUrl(url) => {
            // Open URL using xdg-open and close the screenshot tool
            log::info!("Opening URL: {}", url);
            if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                log::error!("Failed to open URL: {}", e);
            }

            // Close the screenshot tool
            let cmds = app.outputs.iter().map(|o| destroy_layer_surface(o.id));
            let Some(args) = app.screenshot_args.take() else {
                log::error!("Failed to find screenshot Args for OpenUrl message.");
                return cosmic::Task::batch(cmds);
            };
            let Args { tx, .. } = args;
            tokio::spawn(async move {
                if let Err(_err) = tx.send(PortalResponse::Cancelled).await {
                    log::error!("Failed to send screenshot event");
                }
            });

            cosmic::Task::batch(cmds)
        }
        Msg::ToggleSettingsDrawer => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.settings_drawer_open = !args.settings_drawer_open;
            }
            cosmic::Task::none()
        }
        Msg::ToggleMagnifier => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.magnifier_enabled = !args.magnifier_enabled;
                // Persist all settings
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                };
                config.save();
            }
            cosmic::Task::none()
        }
        Msg::SetSaveLocationPictures => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.save_location_setting = SaveLocation::Pictures;
                // Persist all settings
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                };
                config.save();
            }
            cosmic::Task::none()
        }
        Msg::SetSaveLocationDocuments => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.save_location_setting = SaveLocation::Documents;
                // Persist all settings
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                };
                config.save();
            }
            cosmic::Task::none()
        }
        Msg::ToggleCopyOnSave => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.copy_to_clipboard_on_save = !args.copy_to_clipboard_on_save;
                // Persist all settings
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                };
                config.save();
            }
            cosmic::Task::none()
        }
        Msg::SelectRegionMode => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Switch to rectangle selection with a fresh/default rect
                args.choice = Choice::Rectangle(Rect::default(), DragState::default());
                // Clear any previous state
                args.ocr_overlays.clear();
                args.ocr_status = OcrStatus::Idle;
                args.ocr_text = None;
                args.qr_codes.clear();
                args.qr_scanning = false;
                args.arrows.clear();
                args.arrow_mode = false;
                args.arrow_drawing = None;
                args.redactions.clear();
                args.redact_mode = false;
                args.redact_drawing = None;
            }
            cosmic::Task::none()
        }
        Msg::SelectWindowMode => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Get the first output name to use for window mode
                if let Some(output) = app.outputs.first() {
                    args.choice = Choice::Window(output.name.clone(), None);
                    // Clear any previous state
                    args.ocr_overlays.clear();
                    args.ocr_status = OcrStatus::Idle;
                    args.ocr_text = None;
                    args.qr_codes.clear();
                    args.qr_scanning = false;
                    args.arrows.clear();
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    args.redactions.clear();
                    args.redact_mode = false;
                    args.redact_drawing = None;
                }
            }
            cosmic::Task::none()
        }
        Msg::SelectScreenMode => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Select the first output as screen
                if let Some(output) = app.outputs.first() {
                    args.choice = Choice::Output(output.name.clone());
                    // Clear any previous state
                    args.ocr_overlays.clear();
                    args.ocr_status = OcrStatus::Idle;
                    args.ocr_text = None;
                    args.qr_codes.clear();
                    args.qr_scanning = false;
                    args.arrows.clear();
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    args.redactions.clear();
                    args.redact_mode = false;
                    args.redact_drawing = None;
                }
            }
            cosmic::Task::none()
        }
    }
}

pub fn update_args(app: &mut App, args: Args) -> cosmic::Task<crate::app::Msg> {
    let Args {
        handle,
        app_id,
        parent_window,
        options,
        output_images: images,
        tx,
        choice,
        action,
        location,
        toplevel_images,
        qr_codes: _,
        qr_scanning: _,
        ocr_status: _,
        ocr_overlays: _,
        ocr_text: _,
        arrows: _,
        arrow_mode: _,
        arrow_drawing: _,
        redactions: _,
        redact_mode: _,
        redact_drawing: _,
        toolbar_position: _,
        settings_drawer_open: _,
        magnifier_enabled: _,
        save_location_setting: _,
        copy_to_clipboard_on_save: _,
        also_copy_to_clipboard: _,
    } = &args;

    if app.outputs.len() != images.len() {
        log::error!(
            "Screenshot output count mismatch: {} != {}",
            app.outputs.len(),
            images.len()
        );
        log::warn!("Screenshot outputs: {:?}", app.outputs);
        log::warn!("Screenshot images: {:?}", images.keys().collect::<Vec<_>>());
        return cosmic::Task::none();
    }

    // update output bg sources
    if let Ok(c) = cosmic::cosmic_config::Config::new_state(
        cosmic_bg_config::NAME,
        cosmic_bg_config::state::State::version(),
    ) {
        let bg_state = match cosmic_bg_config::state::State::get_entry(&c) {
            Ok(state) => state,
            Err((err, s)) => {
                log::error!("Failed to get bg config state: {:?}", err);
                s
            }
        };
        for o in &mut app.outputs {
            let source = bg_state.wallpapers.iter().find(|s| s.0 == o.name);
            o.bg_source = Some(source.cloned().map(|s| s.1).unwrap_or_else(|| {
                cosmic_bg_config::Source::Path(
                    "/usr/share/backgrounds/pop/kate-hazen-COSMIC-desktop-wallpaper.png".into(),
                )
            }));
        }
    } else {
        log::error!("Failed to get bg config state");
        for o in &mut app.outputs {
            o.bg_source = Some(cosmic_bg_config::Source::Path(
                "/usr/share/backgrounds/pop/kate-hazen-COSMIC-desktop-wallpaper.png".into(),
            ));
        }
    }
    app.location_options = vec![
        fl!("save-to", "clipboard"),
        fl!("save-to", "pictures"),
        fl!("save-to", "documents"),
    ];

    if app.screenshot_args.replace(args).is_none() {
        let cmds: Vec<_> = app
            .outputs
            .iter()
            .map(
                |OutputState {
                     output, id, name, ..
                 }| {
                    get_layer_surface(SctkLayerSurfaceSettings {
                        id: *id,
                        layer: Layer::Overlay,
                        keyboard_interactivity: KeyboardInteractivity::Exclusive,
                        input_zone: None,
                        anchor: Anchor::all(),
                        output: IcedOutput::Output(output.clone()),
                        namespace: "blazingshot".to_string(),
                        size: Some((None, None)),
                        exclusive_zone: -1,
                        size_limits: Limits::NONE.min_height(1.0).min_width(1.0),
                        ..Default::default()
                    })
                },
            )
            .collect();
        cosmic::Task::batch(cmds)
    } else {
        log::info!("Existing screenshot args updated");
        cosmic::Task::none()
    }
}

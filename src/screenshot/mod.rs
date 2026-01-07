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
use crate::config::{BlazingshotConfig, SaveLocation, ShapeColor};
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
        log::debug!(
            "ScreenshotImage captured: {}x{} pixels",
            rgba.width(),
            rgba.height()
        );
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

// Re-export arrow/redact/shape types from the arrow module
pub use crate::arrow::{
    Annotation, ArrowAnnotation, CircleOutlineAnnotation, PixelateAnnotation, RectOutlineAnnotation,
    RedactAnnotation,
};

// Arrow/redact/shape functions are now in crate::arrow module
use crate::arrow::draw_annotations_in_order;

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

// Re-export ShapeTool, RedactTool, and ToolbarPosition from config module
pub use crate::config::ShapeTool;
pub use crate::config::RedactTool;
pub use crate::config::ToolbarPosition;

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
    PixelateModeToggle,                     // toggle pixelate drawing mode
    PixelateStart(f32, f32),                // start drawing pixelate rectangle at position
    PixelateEnd(f32, f32),                  // finish pixelate rectangle at position
    PixelateCancel,                         // cancel current pixelate drawing
    CircleModeToggle,                       // toggle circle/ellipse drawing mode
    CircleStart(f32, f32),                  // start drawing circle at position
    CircleEnd(f32, f32),                    // finish drawing circle at position
    CircleCancel,                           // cancel current circle drawing
    RectOutlineModeToggle,                  // toggle rectangle outline drawing mode
    RectOutlineStart(f32, f32),             // start drawing rectangle outline at position
    RectOutlineEnd(f32, f32),               // finish drawing rectangle outline at position
    RectOutlineCancel,                      // cancel current rectangle outline drawing
    ClearAnnotations,                       // clear all annotations (arrows, redactions, circles, rectangles)
    ShapeModeToggle,                         // toggle shape drawing mode (uses primary_shape_tool)
    SetPrimaryShapeTool(ShapeTool),          // set the primary shape tool (arrow/circle/rectangle)
    CycleShapeTool,                          // cycle to next shape tool and activate it
    ToggleShapePopup,                        // toggle shape mode on/off (normal click)
    OpenShapePopup,                          // open shape popup (right-click or long-press)
    CloseShapePopup,                         // close shape popup without deactivating shape mode (for click-outside)
    SetShapeColor(ShapeColor),               // set shape annotation color
    ToggleShapeShadow,                       // toggle shadow on shapes
    SetPrimaryRedactTool(RedactTool),        // set the primary redact tool (redact/pixelate)
    CycleRedactTool,                         // cycle to next redact tool and activate it
    ToggleRedactPopup,                       // toggle redact mode on/off (normal click)
    OpenRedactPopup,                         // open redact popup (right-click or long-press)
    CloseRedactPopup,                        // close redact popup without deactivating mode
    ClearRedactions,                         // clear all redactions (redact and pixelate)
    SetPixelationBlockSize(u32),             // set pixelation block size (UI only, no save)
    SavePixelationBlockSize,                 // save current pixelation block size to config
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
    SelectWindowMode(usize),                // switch to window selection mode, param is output index (W)
    SelectScreenMode(usize),                // select screen at index (S)
    NavigateLeft,                           // navigate left (prev screen)
    NavigateRight,                          // navigate right (next screen)
    NavigateUp,                             // navigate up (prev window)
    NavigateDown,                           // navigate down (next window)
    ConfirmSelection,                       // confirm current highlight (Space/Enter)
    Undo,                                   // undo last annotation (Ctrl+Z)
    Redo,                                   // redo undone annotation (Ctrl+Y / Ctrl+Shift+Z)
}

#[derive(Debug, Clone)]
pub enum Choice {
    /// Output selection: None = picker mode (selecting), Some = confirmed (screen locked in)
    Output(Option<String>),
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
    /// All annotations in order for undo/redo (unified history)
    pub annotations: Vec<Annotation>,
    /// Current position in the annotations array for undo/redo
    pub annotation_index: usize,
    /// Arrow annotations (derived from annotations array)
    pub arrows: Vec<ArrowAnnotation>,
    /// Whether arrow drawing mode is active
    pub arrow_mode: bool,
    /// Current arrow being drawn (start point set, waiting for end point)
    pub arrow_drawing: Option<(f32, f32)>,
    /// Redaction annotations (derived from annotations array)
    pub redactions: Vec<RedactAnnotation>,
    /// Whether redact drawing mode is active
    pub redact_mode: bool,
    /// Current redaction being drawn (start point set, waiting for end point)
    pub redact_drawing: Option<(f32, f32)>,
    /// Pixelation annotations (derived from annotations array)
    pub pixelations: Vec<PixelateAnnotation>,
    /// Whether pixelate drawing mode is active
    pub pixelate_mode: bool,
    /// Current pixelation being drawn (start point set, waiting for end point)
    pub pixelate_drawing: Option<(f32, f32)>,
    /// Circle/ellipse outline annotations (derived from annotations array)
    pub circles: Vec<CircleOutlineAnnotation>,
    /// Whether circle/ellipse drawing mode is active
    pub circle_mode: bool,
    /// Current circle/ellipse being drawn (start point set, waiting for end point)
    pub circle_drawing: Option<(f32, f32)>,
    /// Rectangle outline annotations (derived from annotations array)
    pub rect_outlines: Vec<RectOutlineAnnotation>,
    /// Whether rectangle outline drawing mode is active
    pub rect_outline_mode: bool,
    /// Current rectangle outline being drawn (start point set, waiting for end point)
    pub rect_outline_drawing: Option<(f32, f32)>,
    /// Toolbar position on screen
    pub toolbar_position: ToolbarPosition,
    /// Whether settings drawer is open
    pub settings_drawer_open: bool,
    /// Primary shape tool shown in the button
    pub primary_shape_tool: ShapeTool,
    /// Whether shape settings popup is open
    pub shape_popup_open: bool,
    /// Current color for shape annotations
    pub shape_color: ShapeColor,
    /// Whether to add shadow/border to shapes
    pub shape_shadow: bool,
    /// Primary redact tool shown in the button
    pub primary_redact_tool: RedactTool,
    /// Whether redact settings popup is open
    pub redact_popup_open: bool,
    /// Pixelation block size (larger = more pixelated)
    pub pixelation_block_size: u32,
    /// Whether magnifier is enabled (persisted setting)
    pub magnifier_enabled: bool,
    /// Save location setting (Pictures or Documents)
    pub save_location_setting: SaveLocation,
    /// Whether to also copy to clipboard when saving (persisted setting)
    pub copy_to_clipboard_on_save: bool,
    /// Whether to also copy to clipboard for the current save operation
    pub also_copy_to_clipboard: bool,
    /// Highlighted window index for keyboard navigation (when in Window mode with None selected)
    pub highlighted_window_index: usize,
    /// Focused output index for keyboard navigation (which screen is active)
    pub focused_output_index: usize,
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
        };

        // Always interactive for blazingshot
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let first_output = &*outputs[0].name;
        let output_images = self
            .interactive_output_images(&outputs, app_id)
            .await
            .unwrap_or_default();
        
        // Log output image sizes for debugging HiDPI
        for (name, img) in &output_images {
            log::debug!(
                "Output image {}: {}x{} pixels",
                name,
                img.rgba.width(),
                img.rgba.height()
            );
        }
        
        let toplevel_images = self
            .interactive_toplevel_images(&outputs)
            .await
            .unwrap_or_default();
        
        // Log toplevel image sizes for debugging HiDPI
        for (output_name, imgs) in &toplevel_images {
            for (i, img) in imgs.iter().enumerate() {
                log::debug!(
                    "Toplevel {} on output {}: {}x{} pixels",
                    i,
                    output_name,
                    img.rgba.width(),
                    img.rgba.height()
                );
            }
        }

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
                annotations: Vec::new(),
                annotation_index: 0,
                arrows: Vec::new(),
                arrow_mode: false,
                arrow_drawing: None,
                redactions: Vec::new(),
                redact_mode: false,
                redact_drawing: None,
                pixelations: Vec::new(),
                pixelate_mode: false,
                pixelate_drawing: None,
                circles: Vec::new(),
                circle_mode: false,
                circle_drawing: None,
                rect_outlines: Vec::new(),
                rect_outline_mode: false,
                rect_outline_drawing: None,
                toolbar_position: config.toolbar_position,
                settings_drawer_open: false,
                primary_shape_tool: config.primary_shape_tool,
                shape_popup_open: false,
                shape_color: config.shape_color,
                shape_shadow: config.shape_shadow,
                primary_redact_tool: config.primary_redact_tool,
                redact_popup_open: false,
                pixelation_block_size: config.pixelation_block_size,
                magnifier_enabled: config.magnifier_enabled,
                save_location_setting: config.save_location,
                copy_to_clipboard_on_save: config.copy_to_clipboard_on_save,
                also_copy_to_clipboard: false,
                highlighted_window_index: 0,
                focused_output_index: 0,
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
            &args.circles,
            args.circle_mode,
            args.circle_drawing,
            Msg::CircleModeToggle,
            Msg::CircleStart,
            Msg::CircleEnd,
            &args.rect_outlines,
            args.rect_outline_mode,
            args.rect_outline_drawing,
            Msg::RectOutlineModeToggle,
            Msg::RectOutlineStart,
            Msg::RectOutlineEnd,
            &args.redactions,
            args.redact_mode,
            args.redact_drawing,
            Msg::RedactModeToggle,
            Msg::RedactStart,
            Msg::RedactEnd,
            &args.pixelations,
            &args.annotations[..args.annotation_index],
            args.pixelate_mode,
            args.pixelate_drawing,
            Msg::PixelateModeToggle,
            Msg::PixelateStart,
            Msg::PixelateEnd,
            Msg::ClearAnnotations,
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
            args.highlighted_window_index,
            args.focused_output_index,
            i,
            args.primary_shape_tool,
            args.shape_popup_open,
            args.shape_color,
            args.shape_shadow,
            Msg::ShapeModeToggle,
            Msg::ToggleShapePopup,
            Msg::OpenShapePopup,
            Msg::CloseShapePopup,
            Msg::SetPrimaryShapeTool,
            Msg::SetShapeColor,
            Msg::ToggleShapeShadow,
            // has_any_annotations for clear button in popup (only shapes, not redactions)
            !args.arrows.is_empty()
                || !args.circles.is_empty()
                || !args.rect_outlines.is_empty(),
            args.primary_redact_tool,
            args.redact_popup_open,
            Msg::ToggleRedactPopup,
            Msg::OpenRedactPopup,
            Msg::CloseRedactPopup,
            Msg::SetPrimaryRedactTool,
            Msg::ClearRedactions,
            // has_any_redactions for clear button in redact popup
            !args.redactions.is_empty() || !args.pixelations.is_empty(),
            args.pixelation_block_size,
            Msg::SetPixelationBlockSize,
            Msg::SavePixelationBlockSize,
            Msg::ConfirmSelection,
            // is_active_output: determines if this screen is where the selection is
            // Other screens should be darkened when there's an active selection
            {
                let output_name = &output.name;
                match &args.choice {
                    // Rectangle mode: ALL screens are active (user can draw new rectangle on any screen)
                    Choice::Rectangle(_, _) => true,
                    // Picker modes: all outputs are "active" (no dimming)
                    Choice::Output(None) | Choice::Window(_, None) => true,
                    // Confirmed window: only that output is active
                    Choice::Window(win_output, Some(_)) => output_name == win_output,
                    // Confirmed screen: only that output is active
                    Choice::Output(Some(selected)) => output_name == selected,
                }
            },
            // has_confirmed_selection: show dark overlay on non-active screens 
            // Only for window and screen modes, NOT for rectangle mode
            {
                match &args.choice {
                    // Rectangle mode: no overlay on other screens (user can draw new rectangle)
                    Choice::Rectangle(_, _) => false,
                    Choice::Window(_, Some(_)) => true,
                    Choice::Output(Some(_)) => true,
                    _ => false,
                }
            },
            Msg::SelectScreenMode(i),
        ),
        {
            // Determine if we have a complete selection for action shortcuts
            let has_selection = match &args.choice {
                Choice::Rectangle(r, _) => r.dimensions().is_some(),
                Choice::Window(_, Some(_)) => true,
                Choice::Output(Some(_)) => true, // Only confirmed screen counts as selection
                _ => false,
            };
            let arrow_mode = args.arrow_mode;
            let redact_mode = args.redact_mode;
            // current_output_index is the screen where this keyboard event is received
            let current_output_index = i;
            // Check if we're in a mode that supports navigation
            let in_window_picker = matches!(&args.choice, Choice::Window(_, None));
            let in_screen_picker = matches!(&args.choice, Choice::Output(None)); // Picker mode only
            // Check if OCR/QR have results (pressing O/Q again should copy and close)
            let has_ocr_result = args.ocr_text.is_some();
            let has_qr_result = !args.qr_codes.is_empty();

            move |key, modifiers| match key {
                // Ctrl+hjkl or Ctrl+arrows: move toolbar position
                Key::Character(c) if c.as_str() == "h" && modifiers.control() => {
                    Some(Msg::ToolbarPositionChange(ToolbarPosition::Left))
                }
                Key::Character(c) if c.as_str() == "j" && modifiers.control() => {
                    Some(Msg::ToolbarPositionChange(ToolbarPosition::Bottom))
                }
                Key::Character(c) if c.as_str() == "k" && modifiers.control() => {
                    Some(Msg::ToolbarPositionChange(ToolbarPosition::Top))
                }
                Key::Character(c) if c.as_str() == "l" && modifiers.control() => {
                    Some(Msg::ToolbarPositionChange(ToolbarPosition::Right))
                }
                Key::Named(Named::ArrowLeft) if modifiers.control() => {
                    Some(Msg::ToolbarPositionChange(ToolbarPosition::Left))
                }
                Key::Named(Named::ArrowDown) if modifiers.control() => {
                    Some(Msg::ToolbarPositionChange(ToolbarPosition::Bottom))
                }
                Key::Named(Named::ArrowUp) if modifiers.control() => {
                    Some(Msg::ToolbarPositionChange(ToolbarPosition::Top))
                }
                Key::Named(Named::ArrowRight) if modifiers.control() => {
                    Some(Msg::ToolbarPositionChange(ToolbarPosition::Right))
                }
                // Undo/redo shortcuts
                Key::Character(c) if c.as_str() == "z" && modifiers.control() && !modifiers.shift() => {
                    Some(Msg::Undo)
                }
                Key::Character(c) if (c.as_str() == "y" && modifiers.control()) 
                    || (c.as_str() == "z" && modifiers.control() && modifiers.shift()) => {
                    Some(Msg::Redo)
                }
                // Save/copy shortcuts (always available - empty selection captures all screens)
                Key::Named(Named::Enter) if modifiers.control() => Some(Msg::SaveToPictures),
                Key::Named(Named::Escape) => Some(Msg::Cancel),
                // Space/Enter to confirm selection in picker mode (window or screen)
                Key::Named(Named::Space) if in_window_picker || in_screen_picker => Some(Msg::ConfirmSelection),
                Key::Named(Named::Enter) if in_window_picker || in_screen_picker => Some(Msg::ConfirmSelection),
                // Enter to copy when not in picker mode
                Key::Named(Named::Enter) => Some(Msg::CopyToClipboard),
                // Navigation keys in window picker: hjkl and arrows all navigate windows
                Key::Character(c) if c.as_str() == "h" && in_window_picker => {
                    Some(Msg::NavigateUp)
                }
                Key::Character(c) if c.as_str() == "l" && in_window_picker => {
                    Some(Msg::NavigateDown)
                }
                Key::Character(c) if c.as_str() == "j" && in_window_picker => {
                    Some(Msg::NavigateDown)
                }
                Key::Character(c) if c.as_str() == "k" && in_window_picker => {
                    Some(Msg::NavigateUp)
                }
                // Navigation keys in screen picker: h/l and arrows navigate screens
                Key::Character(c) if c.as_str() == "h" && in_screen_picker => {
                    Some(Msg::NavigateLeft)
                }
                Key::Character(c) if c.as_str() == "l" && in_screen_picker => {
                    Some(Msg::NavigateRight)
                }
                Key::Named(Named::ArrowLeft) if in_window_picker => Some(Msg::NavigateUp),
                Key::Named(Named::ArrowRight) if in_window_picker => Some(Msg::NavigateDown),
                Key::Named(Named::ArrowUp) if in_window_picker => Some(Msg::NavigateUp),
                Key::Named(Named::ArrowDown) if in_window_picker => Some(Msg::NavigateDown),
                Key::Named(Named::ArrowLeft) if in_screen_picker => Some(Msg::NavigateLeft),
                Key::Named(Named::ArrowRight) if in_screen_picker => Some(Msg::NavigateRight),
                // Mode toggle shortcuts (require selection)
                // Shift+A: cycle shape tool (arrow -> circle -> rectangle -> arrow)
                Key::Character(c)
                    if c.as_str().eq_ignore_ascii_case("a")
                        && modifiers.shift()
                        && has_selection =>
                {
                    Some(Msg::CycleShapeTool)
                }
                // A: toggle current shape tool
                Key::Character(c) if c.as_str() == "a" && has_selection => {
                    Some(Msg::ShapeModeToggle)
                }
                // Shift+D: cycle to next redact tool (redact/pixelate) and activate it
                Key::Character(c)
                    if c.as_str() == "D"
                        && modifiers.shift()
                        && has_selection =>
                {
                    Some(Msg::CycleRedactTool)
                }
                // D: toggle current redact tool
                Key::Character(c) if c.as_str() == "d" && has_selection => {
                    Some(Msg::ToggleRedactPopup)
                }
                // OCR shortcut: if result exists, copy and close; otherwise start OCR
                Key::Character(c) if c.as_str() == "o" && has_ocr_result => {
                    Some(Msg::OcrCopyAndClose)
                }
                Key::Character(c) if c.as_str() == "o" && has_selection => {
                    Some(Msg::OcrRequested)
                }
                // QR shortcut: if result exists, copy and close; otherwise start scan
                Key::Character(c) if c.as_str() == "q" && has_qr_result => {
                    Some(Msg::QrCopyAndClose)
                }
                Key::Character(c) if c.as_str() == "q" && has_selection => {
                    Some(Msg::QrRequested)
                }
                // Selection mode shortcuts (always available, but not when in draw mode)
                // Use current_output_index (the screen where this key was pressed)
                Key::Character(c) if c.as_str() == "r" && !arrow_mode && !redact_mode => {
                    Some(Msg::SelectRegionMode)
                }
                Key::Character(c) if c.as_str() == "w" && !arrow_mode && !redact_mode => {
                    Some(Msg::SelectWindowMode(current_output_index))
                }
                Key::Character(c) if c.as_str() == "s" && !arrow_mode && !redact_mode => {
                    Some(Msg::SelectScreenMode(current_output_index))
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
                annotations,
                annotation_index,
                also_copy_to_clipboard,
                ..
            } = args;
            // Only use annotations up to annotation_index (respects undo)
            let annotations = &annotations[..annotation_index];

            let mut success = true;
            let image_path = Screenshot::get_img_path(location);

            match choice {
                Choice::Output(Some(output_name)) => {
                    if let Some(img) = images.remove(&output_name) {
                        let mut final_img = img.rgba.clone();

                        // Draw annotations (they are in global coords, output_rect is also global)
                        if !annotations.is_empty() {
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
                                // Draw all annotations in order
                                draw_annotations_in_order(
                                    &mut final_img,
                                    &annotations,
                                    &output_rect,
                                    scale,
                                );
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

                        // Draw annotations onto the final image
                        if !annotations.is_empty() {
                            draw_annotations_in_order(&mut img, &annotations, &r, target_scale);
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

                        // Draw annotations if any
                        if !annotations.is_empty() {
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
                                // Annotation coords are stored in global coordinates (output.left + pos.x)
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
                                draw_annotations_in_order(
                                    &mut final_img,
                                    &annotations,
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
                            args.pixelations.clear();
                            args.redact_mode = false;
                            args.redact_drawing = None;
                            args.pixelate_mode = false;
                            args.pixelate_drawing = None;
                            args.circles.clear();
                            args.circle_mode = false;
                            args.circle_drawing = None;
                            args.rect_outlines.clear();
                            args.rect_outline_mode = false;
                            args.rect_outline_drawing = None;
                            args.annotations.clear();
                            args.annotation_index = 0;
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
                        args.pixelations.clear();
                        args.redact_mode = false;
                        args.redact_drawing = None;
                        args.pixelate_mode = false;
                        args.pixelate_drawing = None;
                        args.circles.clear();
                        args.circle_mode = false;
                        args.circle_drawing = None;
                        args.rect_outlines.clear();
                        args.rect_outline_mode = false;
                        args.rect_outline_drawing = None;
                        args.annotations.clear();
                        args.annotation_index = 0;
                    }
                }
                // Clear arrows/redactions when switching modes (Region, Window, or Output picker)
                if matches!(
                    &c,
                    Choice::Rectangle(_, DragState::None)
                        | Choice::Window(_, None)
                        | Choice::Output(None) // Only clear in picker mode, not when confirmed
                ) {
                    args.arrows.clear();
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    args.redactions.clear();
                    args.pixelations.clear();
                    args.redact_mode = false;
                    args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
                    args.circles.clear();
                    args.circle_mode = false;
                    args.circle_drawing = None;
                    args.rect_outlines.clear();
                    args.rect_outline_mode = false;
                    args.rect_outline_drawing = None;
                    args.annotations.clear();
                    args.annotation_index = 0;
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
            // In screen picker mode, cursor hover just updates focused_output_index
            // In confirmed mode, this is ignored (screen stays locked)
            if let Some(args) = app.screenshot_args.as_mut() {
                // Find the output index
                if let Some(output_index) = app.outputs.iter().position(|o| o.output == wl_output)
                {
                    // Only update highlight in picker mode (None means picker)
                    if matches!(args.choice, Choice::Output(None)) {
                        args.focused_output_index = output_index;
                    }
                }
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
                                args.pixelations.clear();
                args.redact_mode = false;
                args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
                args.circles.clear();
                args.circle_mode = false;
                args.circle_drawing = None;
                args.rect_outlines.clear();
                args.rect_outline_mode = false;
                args.rect_outline_drawing = None;
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
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
            }

            // Get the selection and run QR detection on that area
            if let Some(args) = app.screenshot_args.as_ref() {
                // Only use annotations up to annotation_index (respects undo)
                let annotations = args.annotations[..args.annotation_index].to_vec();
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
                    Choice::Output(Some(output_name)) => {
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
                    // Apply annotations to the image before QR scanning
                    if !annotations.is_empty() {
                        draw_annotations_in_order(&mut cropped, &annotations, &selection_rect, scale);
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
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
            }

            // Get the selection and run OCR on that area
            if let Some(args) = app.screenshot_args.as_ref() {
                // Only use annotations up to annotation_index (respects undo)
                let annotations = args.annotations[..args.annotation_index].to_vec();
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
                    Choice::Output(Some(output_name)) => {
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
                    // Apply annotations to the image before OCR
                    if !annotations.is_empty() {
                        draw_annotations_in_order(
                            &mut cropped_img,
                            &annotations,
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
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
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
                let arrow = ArrowAnnotation {
                    start_x,
                    start_y,
                    end_x: x,
                    end_y: y,
                    color: args.shape_color,
                    shadow: args.shape_shadow,
                };
                args.arrows.push(arrow.clone());
                // Add to unified annotations and truncate any redo history
                args.annotations.truncate(args.annotation_index);
                args.annotations.push(Annotation::Arrow(arrow));
                args.annotation_index = args.annotations.len();
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
                let redact = RedactAnnotation {
                    x: start_x,
                    y: start_y,
                    x2: x,
                    y2: y,
                };
                args.redactions.push(redact.clone());
                // Add to unified annotations and truncate any redo history
                args.annotations.truncate(args.annotation_index);
                args.annotations.push(Annotation::Redact(redact));
                args.annotation_index = args.annotations.len();
            }
            cosmic::Task::none()
        }
        Msg::RedactCancel => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.redact_drawing = None;
            }
            cosmic::Task::none()
        }
        Msg::PixelateModeToggle => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.pixelate_mode = !args.pixelate_mode;
                // Cancel any in-progress pixelation when toggling off
                if !args.pixelate_mode {
                    args.pixelate_drawing = None;
                } else {
                    // Disable other modes when enabling pixelate mode
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    args.redact_mode = false;
                    args.redact_drawing = None;
                    args.circle_mode = false;
                    args.circle_drawing = None;
                    args.rect_outline_mode = false;
                    args.rect_outline_drawing = None;
                    // Close shape popup
                    args.shape_popup_open = false;
                    // Clear OCR/QR
                    args.ocr_overlays.clear();
                    args.ocr_status = OcrStatus::Idle;
                    args.ocr_text = None;
                    args.qr_codes.clear();
                }
            }
            cosmic::Task::none()
        }
        Msg::PixelateStart(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && args.pixelate_mode
            {
                args.pixelate_drawing = Some((x, y));
            }
            cosmic::Task::none()
        }
        Msg::PixelateEnd(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && let Some((start_x, start_y)) = args.pixelate_drawing.take()
            {
                let pixelate = PixelateAnnotation {
                    x: start_x,
                    y: start_y,
                    x2: x,
                    y2: y,
                    block_size: args.pixelation_block_size,
                };
                args.pixelations.push(pixelate.clone());
                // Add to unified annotations and truncate any redo history
                args.annotations.truncate(args.annotation_index);
                args.annotations.push(Annotation::Pixelate(pixelate));
                args.annotation_index = args.annotations.len();
            }
            cosmic::Task::none()
        }
        Msg::PixelateCancel => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.pixelate_drawing = None;
            }
            cosmic::Task::none()
        }
        Msg::CircleModeToggle => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.circle_mode = !args.circle_mode;
                if !args.circle_mode {
                    args.circle_drawing = None;
                } else {
                    // Mutually exclusive with other draw modes
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    args.redact_mode = false;
                    args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
                    args.rect_outline_mode = false;
                    args.rect_outline_drawing = None;
                    // Clear OCR/QR when enabling draw mode
                    args.ocr_overlays.clear();
                    args.ocr_status = OcrStatus::Idle;
                    args.ocr_text = None;
                    args.qr_codes.clear();
                }
            }
            cosmic::Task::none()
        }
        Msg::CircleStart(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && args.circle_mode
            {
                args.circle_drawing = Some((x, y));
            }
            cosmic::Task::none()
        }
        Msg::CircleEnd(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && let Some((start_x, start_y)) = args.circle_drawing.take()
            {
                let circle = CircleOutlineAnnotation {
                    start_x,
                    start_y,
                    end_x: x,
                    end_y: y,
                    color: args.shape_color,
                    shadow: args.shape_shadow,
                };
                args.circles.push(circle.clone());
                // Add to unified annotations and truncate any redo history
                args.annotations.truncate(args.annotation_index);
                args.annotations.push(Annotation::Circle(circle));
                args.annotation_index = args.annotations.len();
            }
            cosmic::Task::none()
        }
        Msg::CircleCancel => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.circle_drawing = None;
            }
            cosmic::Task::none()
        }
        Msg::RectOutlineModeToggle => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.rect_outline_mode = !args.rect_outline_mode;
                if !args.rect_outline_mode {
                    args.rect_outline_drawing = None;
                } else {
                    // Mutually exclusive with other draw modes
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    args.redact_mode = false;
                    args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
                    args.circle_mode = false;
                    args.circle_drawing = None;
                    // Clear OCR/QR when enabling draw mode
                    args.ocr_overlays.clear();
                    args.ocr_status = OcrStatus::Idle;
                    args.ocr_text = None;
                    args.qr_codes.clear();
                }
            }
            cosmic::Task::none()
        }
        Msg::RectOutlineStart(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && args.rect_outline_mode
            {
                args.rect_outline_drawing = Some((x, y));
            }
            cosmic::Task::none()
        }
        Msg::RectOutlineEnd(x, y) => {
            if let Some(args) = app.screenshot_args.as_mut()
                && let Some((start_x, start_y)) = args.rect_outline_drawing.take()
            {
                let rect = RectOutlineAnnotation {
                    start_x,
                    start_y,
                    end_x: x,
                    end_y: y,
                    color: args.shape_color,
                    shadow: args.shape_shadow,
                };
                args.rect_outlines.push(rect.clone());
                // Add to unified annotations and truncate any redo history
                args.annotations.truncate(args.annotation_index);
                args.annotations.push(Annotation::Rectangle(rect));
                args.annotation_index = args.annotations.len();
            }
            cosmic::Task::none()
        }
        Msg::RectOutlineCancel => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.rect_outline_drawing = None;
            }
            cosmic::Task::none()
        }
        Msg::ClearAnnotations => {
            // Clear only shape annotations (arrows, circles, rectangles) - NOT redactions
            if let Some(args) = app.screenshot_args.as_mut() {
                args.arrows.clear();
                args.arrow_drawing = None;
                args.arrow_mode = false;
                args.circles.clear();
                args.circle_drawing = None;
                args.circle_mode = false;
                args.rect_outlines.clear();
                args.rect_outline_drawing = None;
                args.rect_outline_mode = false;
            }
            cosmic::Task::none()
        }
        Msg::ToolbarPositionChange(position) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.toolbar_position = position;
                // Persist the setting
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
                };
                config.save();
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
                // Close other popups if opening settings drawer
                if args.settings_drawer_open {
                    args.shape_popup_open = false;
                    args.redact_popup_open = false;
                }
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
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
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
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
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
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
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
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
                };
                config.save();
            }
            cosmic::Task::none()
        }
        Msg::ShapeModeToggle => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Toggle the current shape mode based on primary_shape_tool
                match args.primary_shape_tool {
                    ShapeTool::Arrow => {
                        args.arrow_mode = !args.arrow_mode;
                        if !args.arrow_mode {
                            args.arrow_drawing = None;
                        } else {
                            // Disable other modes
                            args.circle_mode = false;
                            args.circle_drawing = None;
                            args.rect_outline_mode = false;
                            args.rect_outline_drawing = None;
                            args.redact_mode = false;
                            args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
                            // Clear OCR/QR
                            args.ocr_overlays.clear();
                            args.ocr_status = OcrStatus::Idle;
                            args.ocr_text = None;
                            args.qr_codes.clear();
                        }
                    }
                    ShapeTool::Circle => {
                        args.circle_mode = !args.circle_mode;
                        if !args.circle_mode {
                            args.circle_drawing = None;
                        } else {
                            args.arrow_mode = false;
                            args.arrow_drawing = None;
                            args.rect_outline_mode = false;
                            args.rect_outline_drawing = None;
                            args.redact_mode = false;
                            args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
                            args.ocr_overlays.clear();
                            args.ocr_status = OcrStatus::Idle;
                            args.ocr_text = None;
                            args.qr_codes.clear();
                        }
                    }
                    ShapeTool::Rectangle => {
                        args.rect_outline_mode = !args.rect_outline_mode;
                        if !args.rect_outline_mode {
                            args.rect_outline_drawing = None;
                        } else {
                            args.arrow_mode = false;
                            args.arrow_drawing = None;
                            args.circle_mode = false;
                            args.circle_drawing = None;
                            args.redact_mode = false;
                            args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
                            args.ocr_overlays.clear();
                            args.ocr_status = OcrStatus::Idle;
                            args.ocr_text = None;
                            args.qr_codes.clear();
                        }
                    }
                }
                // Close shape popup if open
                args.shape_popup_open = false;
            }
            cosmic::Task::none()
        }
        Msg::SetPrimaryShapeTool(tool) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.primary_shape_tool = tool;

                // Also activate the selected shape mode
                match tool {
                    ShapeTool::Arrow => {
                        args.arrow_mode = true;
                        args.circle_mode = false;
                        args.circle_drawing = None;
                        args.rect_outline_mode = false;
                        args.rect_outline_drawing = None;
                    }
                    ShapeTool::Circle => {
                        args.circle_mode = true;
                        args.arrow_mode = false;
                        args.arrow_drawing = None;
                        args.rect_outline_mode = false;
                        args.rect_outline_drawing = None;
                    }
                    ShapeTool::Rectangle => {
                        args.rect_outline_mode = true;
                        args.arrow_mode = false;
                        args.arrow_drawing = None;
                        args.circle_mode = false;
                        args.circle_drawing = None;
                    }
                }

                // Keep popup open to allow changing other settings
                // Persist the setting
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
                };
                config.save();
            }
            cosmic::Task::none()
        }
        Msg::CycleShapeTool => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Cycle to next shape tool and activate it
                args.primary_shape_tool = args.primary_shape_tool.next();
                args.shape_popup_open = false;
                // Persist the setting
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
                };
                config.save();
            }
            // Also toggle the shape mode on
            update_msg(app, Msg::ShapeModeToggle)
        }
        Msg::ToggleShapePopup => {
            // Normal click: just toggle shape mode on/off (no popup)
            if let Some(args) = app.screenshot_args.as_mut() {
                let is_shape_active =
                    args.arrow_mode || args.circle_mode || args.rect_outline_mode;

                if is_shape_active {
                    // Shape mode is active -> deactivate it, close popup
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    args.circle_mode = false;
                    args.circle_drawing = None;
                    args.rect_outline_mode = false;
                    args.rect_outline_drawing = None;
                    args.shape_popup_open = false;
                } else {
                    // Shape mode is inactive -> activate it (no popup)
                    args.settings_drawer_open = false;
                    args.shape_popup_open = false; // Close popup if open
                    args.redact_popup_open = false; // Close other popups

                    // Disable other modes first
                    args.redact_mode = false;
                    args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
                    args.ocr_overlays.clear();
                    args.ocr_status = OcrStatus::Idle;
                    args.ocr_text = None;
                    args.qr_codes.clear();

                    // Enable the current shape tool
                    match args.primary_shape_tool {
                        ShapeTool::Arrow => {
                            args.arrow_mode = true;
                            args.circle_mode = false;
                            args.circle_drawing = None;
                            args.rect_outline_mode = false;
                            args.rect_outline_drawing = None;
                        }
                        ShapeTool::Circle => {
                            args.circle_mode = true;
                            args.arrow_mode = false;
                            args.arrow_drawing = None;
                            args.rect_outline_mode = false;
                            args.rect_outline_drawing = None;
                        }
                        ShapeTool::Rectangle => {
                            args.rect_outline_mode = true;
                            args.arrow_mode = false;
                            args.arrow_drawing = None;
                            args.circle_mode = false;
                            args.circle_drawing = None;
                        }
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::OpenShapePopup => {
            // Right-click or long-press: toggle popup (close if open, open if closed)
            if let Some(args) = app.screenshot_args.as_mut() {
                if args.shape_popup_open {
                    // Already open, just close it
                    args.shape_popup_open = false;
                    return cosmic::Task::none();
                }

                args.shape_popup_open = true;
                args.settings_drawer_open = false;
                args.redact_popup_open = false; // Close other popups

                // Disable other modes first
                args.redact_mode = false;
                args.redact_drawing = None;
                args.pixelate_mode = false;
                args.pixelate_drawing = None;
                args.ocr_overlays.clear();
                args.ocr_status = OcrStatus::Idle;
                args.ocr_text = None;
                args.qr_codes.clear();

                // Enable the current shape tool if not already active
                let is_shape_active =
                    args.arrow_mode || args.circle_mode || args.rect_outline_mode;
                if !is_shape_active {
                    match args.primary_shape_tool {
                        ShapeTool::Arrow => {
                            args.arrow_mode = true;
                            args.circle_mode = false;
                            args.circle_drawing = None;
                            args.rect_outline_mode = false;
                            args.rect_outline_drawing = None;
                        }
                        ShapeTool::Circle => {
                            args.circle_mode = true;
                            args.arrow_mode = false;
                            args.arrow_drawing = None;
                            args.rect_outline_mode = false;
                            args.rect_outline_drawing = None;
                        }
                        ShapeTool::Rectangle => {
                            args.rect_outline_mode = true;
                            args.arrow_mode = false;
                            args.arrow_drawing = None;
                            args.circle_mode = false;
                            args.circle_drawing = None;
                        }
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::CloseShapePopup => {
            // Just close the popup without deactivating shape mode (used for click-outside)
            if let Some(args) = app.screenshot_args.as_mut() {
                args.shape_popup_open = false;
            }
            cosmic::Task::none()
        }
        Msg::SetShapeColor(color) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.shape_color = color;
                // Persist the setting
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
                };
                config.save();
            }
            cosmic::Task::none()
        }
        Msg::ToggleShapeShadow => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.shape_shadow = !args.shape_shadow;
                // Persist the setting
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
                };
                config.save();
            }
            cosmic::Task::none()
        }
        Msg::SetPrimaryRedactTool(tool) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.primary_redact_tool = tool;
                // Close the popup after selection
                args.redact_popup_open = false;
                // Activate the selected tool
                match tool {
                    RedactTool::Redact => {
                        args.redact_mode = true;
                        args.pixelate_mode = false;
                    }
                    RedactTool::Pixelate => {
                        args.pixelate_mode = true;
                        args.redact_mode = false;
                    }
                }
                // Disable other modes
                args.arrow_mode = false;
                args.arrow_drawing = None;
                args.circle_mode = false;
                args.circle_drawing = None;
                args.rect_outline_mode = false;
                args.rect_outline_drawing = None;
                args.shape_popup_open = false;
                // Persist the setting
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
                };
                config.save();
            }
            cosmic::Task::none()
        }
        Msg::CycleRedactTool => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.primary_redact_tool = args.primary_redact_tool.next();
                // Persist the setting
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
                };
                config.save();
            }
            // Also activate the new redact mode
            update_msg(app, Msg::ToggleRedactPopup)
        }
        Msg::ToggleRedactPopup => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Check if redact mode is currently active
                let is_active = match args.primary_redact_tool {
                    RedactTool::Redact => args.redact_mode,
                    RedactTool::Pixelate => args.pixelate_mode,
                };

                if is_active {
                    // Turn off the mode
                    args.redact_mode = false;
                    args.pixelate_mode = false;
                    args.redact_drawing = None;
                    args.pixelate_drawing = None;
                    args.redact_popup_open = false;
                } else {
                    // Turn on the mode based on primary tool
                    match args.primary_redact_tool {
                        RedactTool::Redact => {
                            args.redact_mode = true;
                            args.pixelate_mode = false;
                        }
                        RedactTool::Pixelate => {
                            args.pixelate_mode = true;
                            args.redact_mode = false;
                        }
                    }
                    // Disable other modes
                    args.arrow_mode = false;
                    args.arrow_drawing = None;
                    args.circle_mode = false;
                    args.circle_drawing = None;
                    args.rect_outline_mode = false;
                    args.rect_outline_drawing = None;
                    args.shape_popup_open = false;
                    // Close the redact popup on normal click (don't open it)
                    args.redact_popup_open = false;
                    // Close settings drawer if open
                    args.settings_drawer_open = false;
                }
            }
            cosmic::Task::none()
        }
        Msg::OpenRedactPopup => {
            // Right-click or long-press: toggle popup (close if open, open if closed)
            if let Some(args) = app.screenshot_args.as_mut() {
                if args.redact_popup_open {
                    // Already open, just close it
                    args.redact_popup_open = false;
                    return cosmic::Task::none();
                }

                args.redact_popup_open = true;
                // Close other popups
                args.shape_popup_open = false;
                args.settings_drawer_open = false;
            }
            cosmic::Task::none()
        }
        Msg::CloseRedactPopup => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.redact_popup_open = false;
            }
            cosmic::Task::none()
        }
        Msg::ClearRedactions => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.redactions.clear();
                args.pixelations.clear();
            }
            cosmic::Task::none()
        }
        Msg::SetPixelationBlockSize(size) => {
            // Just update the UI value, don't save to disk (too slow for real-time slider)
            if let Some(args) = app.screenshot_args.as_mut() {
                args.pixelation_block_size = size.clamp(4, 64);
            }
            cosmic::Task::none()
        }
        Msg::SavePixelationBlockSize => {
            // Save the current pixelation block size to config (called on slider release)
            if let Some(args) = app.screenshot_args.as_ref() {
                let config = BlazingshotConfig {
                    magnifier_enabled: args.magnifier_enabled,
                    save_location: args.save_location_setting,
                    copy_to_clipboard_on_save: args.copy_to_clipboard_on_save,
                    primary_shape_tool: args.primary_shape_tool,
                    shape_color: args.shape_color,
                    shape_shadow: args.shape_shadow,
                    primary_redact_tool: args.primary_redact_tool,
                    pixelation_block_size: args.pixelation_block_size,
                    toolbar_position: args.toolbar_position,
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
                                args.pixelations.clear();
                args.redact_mode = false;
                args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
            }
            cosmic::Task::none()
        }
        Msg::SelectWindowMode(output_index) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Get the output name from the index
                if let Some(output) = app.outputs.get(output_index) {
                    args.choice = Choice::Window(output.name.clone(), None);
                    args.focused_output_index = output_index;
                    args.highlighted_window_index = 0;
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
                                args.pixelations.clear();
                    args.redact_mode = false;
                    args.redact_drawing = None;
                    args.pixelate_mode = false;
                    args.pixelate_drawing = None;
                }
            }
            cosmic::Task::none()
        }
        Msg::SelectScreenMode(output_index) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Go to picker mode (None), not directly selecting a screen
                args.choice = Choice::Output(None);
                args.focused_output_index = output_index;
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
                args.pixelations.clear();
                args.redact_mode = false;
                args.redact_drawing = None;
                args.pixelate_mode = false;
                args.pixelate_drawing = None;
                args.circles.clear();
                args.circle_mode = false;
                args.circle_drawing = None;
                args.rect_outlines.clear();
                args.rect_outline_mode = false;
                args.rect_outline_drawing = None;
                args.annotations.clear();
                args.annotation_index = 0;
            }
            cosmic::Task::none()
        }
        Msg::NavigateLeft => {
            if let Some(args) = app.screenshot_args.as_mut() {
                let output_count = app.outputs.len();
                if output_count > 0 {
                    match &args.choice {
                        Choice::Window(_, None) => {
                            // In window picker mode: navigate through windows across screens
                            // Get current window count
                            let current_window_count = app
                                .outputs
                                .get(args.focused_output_index)
                                .and_then(|o| args.toplevel_images.get(&o.name))
                                .map(|v| v.len())
                                .unwrap_or(0);

                            if args.highlighted_window_index > 0 {
                                // Move to previous window on same screen
                                args.highlighted_window_index -= 1;
                            } else {
                                // Move to previous screen and select its last window
                                // Keep going left until we find a screen with windows or wrap around
                                let start_index = args.focused_output_index;
                                loop {
                                    args.focused_output_index = if args.focused_output_index == 0 {
                                        output_count - 1
                                    } else {
                                        args.focused_output_index - 1
                                    };

                                    let window_count = app
                                        .outputs
                                        .get(args.focused_output_index)
                                        .and_then(|o| args.toplevel_images.get(&o.name))
                                        .map(|v| v.len())
                                        .unwrap_or(0);

                                    if window_count > 0 {
                                        // Found a screen with windows, select the last one
                                        args.highlighted_window_index = window_count - 1;
                                        if let Some(output) = app.outputs.get(args.focused_output_index) {
                                            args.choice = Choice::Window(output.name.clone(), None);
                                        }
                                        break;
                                    }

                                    // If we've checked all screens and found none with windows,
                                    // stay on current screen
                                    if args.focused_output_index == start_index {
                                        args.highlighted_window_index = 0;
                                        break;
                                    }
                                }
                            }
                        }
                        Choice::Output(None) => {
                            // In screen picker mode: move to previous screen (just update index)
                            args.focused_output_index = if args.focused_output_index == 0 {
                                output_count - 1
                            } else {
                                args.focused_output_index - 1
                            };
                            // Choice stays as None (picker mode)
                        }
                        _ => {}
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::NavigateRight => {
            if let Some(args) = app.screenshot_args.as_mut() {
                let output_count = app.outputs.len();
                if output_count > 0 {
                    match &args.choice {
                        Choice::Window(_, None) => {
                            // In window picker mode: navigate through windows across screens
                            let current_window_count = app
                                .outputs
                                .get(args.focused_output_index)
                                .and_then(|o| args.toplevel_images.get(&o.name))
                                .map(|v| v.len())
                                .unwrap_or(0);

                            if current_window_count > 0
                                && args.highlighted_window_index < current_window_count - 1
                            {
                                // Move to next window on same screen
                                args.highlighted_window_index += 1;
                            } else {
                                // Move to next screen and select its first window
                                // Keep going right until we find a screen with windows or wrap around
                                let start_index = args.focused_output_index;
                                loop {
                                    args.focused_output_index =
                                        (args.focused_output_index + 1) % output_count;

                                    let window_count = app
                                        .outputs
                                        .get(args.focused_output_index)
                                        .and_then(|o| args.toplevel_images.get(&o.name))
                                        .map(|v| v.len())
                                        .unwrap_or(0);

                                    if window_count > 0 {
                                        // Found a screen with windows, select the first one
                                        args.highlighted_window_index = 0;
                                        if let Some(output) = app.outputs.get(args.focused_output_index) {
                                            args.choice = Choice::Window(output.name.clone(), None);
                                        }
                                        break;
                                    }

                                    // If we've checked all screens and found none with windows,
                                    // stay on current screen
                                    if args.focused_output_index == start_index {
                                        args.highlighted_window_index = 0;
                                        break;
                                    }
                                }
                            }
                        }
                        Choice::Output(None) => {
                            // In screen picker mode: move to next screen (just update index)
                            args.focused_output_index = (args.focused_output_index + 1) % output_count;
                            // Choice stays as None (picker mode)
                        }
                        _ => {}
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::NavigateUp => {
            // Same as NavigateLeft for window picker mode
            update_msg(app, Msg::NavigateLeft)
        }
        Msg::NavigateDown => {
            // Same as NavigateRight for window picker mode
            update_msg(app, Msg::NavigateRight)
        }
        Msg::ConfirmSelection => {
            if let Some(args) = app.screenshot_args.as_mut() {
                match &args.choice {
                    Choice::Window(_, None) => {
                        // Confirm the highlighted window on the focused output
                        if let Some(output) = app.outputs.get(args.focused_output_index) {
                            let window_count = args
                                .toplevel_images
                                .get(&output.name)
                                .map(|v| v.len())
                                .unwrap_or(0);
                            if window_count > 0 && args.highlighted_window_index < window_count {
                                args.choice = Choice::Window(
                                    output.name.clone(),
                                    Some(args.highlighted_window_index),
                                );
                            }
                        }
                    }
                    Choice::Output(None) => {
                        // Confirm the highlighted screen (enter confirmed mode)
                        if let Some(output) = app.outputs.get(args.focused_output_index) {
                            args.choice = Choice::Output(Some(output.name.clone()));
                        }
                    }
                    _ => {}
                }
            }
            cosmic::Task::none()
        }
        Msg::Undo => {
            if let Some(args) = app.screenshot_args.as_mut() {
                if args.annotation_index > 0 {
                    args.annotation_index -= 1;
                    // Rebuild the individual arrays from active annotations
                    rebuild_annotation_arrays(args);
                }
            }
            cosmic::Task::none()
        }
        Msg::Redo => {
            if let Some(args) = app.screenshot_args.as_mut() {
                if args.annotation_index < args.annotations.len() {
                    args.annotation_index += 1;
                    // Rebuild the individual arrays from active annotations
                    rebuild_annotation_arrays(args);
                }
            }
            cosmic::Task::none()
        }
    }
}

/// Rebuild the individual annotation arrays from the unified annotations array
/// based on the current annotation_index (for undo/redo support)
fn rebuild_annotation_arrays(args: &mut Args) {
    args.arrows.clear();
    args.circles.clear();
    args.rect_outlines.clear();
    args.redactions.clear();
    args.pixelations.clear();

    for annotation in args.annotations.iter().take(args.annotation_index) {
        match annotation {
            Annotation::Arrow(a) => args.arrows.push(a.clone()),
            Annotation::Circle(c) => args.circles.push(c.clone()),
            Annotation::Rectangle(r) => args.rect_outlines.push(r.clone()),
            Annotation::Redact(r) => args.redactions.push(r.clone()),
            Annotation::Pixelate(p) => args.pixelations.push(p.clone()),
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
        annotations: _,
        annotation_index: _,
        arrows: _,
        arrow_mode: _,
        arrow_drawing: _,
        redactions: _,
        redact_mode: _,
        redact_drawing: _,
        pixelations: _,
        pixelate_mode: _,
        pixelate_drawing: _,
        toolbar_position: _,
        settings_drawer_open: _,
        magnifier_enabled: _,
        save_location_setting: _,
        copy_to_clipboard_on_save: _,
        also_copy_to_clipboard: _,
        highlighted_window_index: _,
        focused_output_index: _,
        circles: _,
        circle_mode: _,
        circle_drawing: _,
        rect_outlines: _,
        rect_outline_mode: _,
        rect_outline_drawing: _,
        primary_shape_tool: _,
        shape_popup_open: _,
        shape_color: _,
        shape_shadow: _,
        primary_redact_tool: _,
        redact_popup_open: _,
        pixelation_block_size: _,
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

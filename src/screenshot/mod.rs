use cosmic::cosmic_config::CosmicConfigEntry;
use cosmic::iced::clipboard::mime::AsMimeTypes;
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
use std::borrow::Cow;
use std::{collections::HashMap, io, path::PathBuf};
use tokio::sync::mpsc::Sender;

use wayland_client::protocol::wl_output::WlOutput;
use zbus::zvariant;

pub use crate::domain::{Action, Choice, DragState, ImageSaveLocation, Rect, RectDimension};
use crate::capture::image::ScreenshotImage;
use crate::capture::ocr::{
    OcrMapping, OcrStatus, is_tesseract_available, models_need_download, run_ocr_on_image_with_status,
};
use crate::capture::qr::{DetectedQrCode, detect_qr_codes_at_resolution, is_duplicate_qr};
use crate::config::{
    SnapPeaConfig, SaveLocation,
};
use crate::core::app::{App, OutputState};
use crate::core::portal::PortalResponse;
use crate::render::image::draw_annotations_in_order;
use crate::session::messages::{
    CaptureMsg, DetectMsg, Direction, DrawMsg, Msg, OcrMsg, QrMsg, SaveLocationChoice, SelectMsg,
    SettingsMsg, ToolMsg,
};
use crate::session::state::{
    AnnotationState, CaptureData, DetectionState, PortalContext, SessionState, UiState,
};
use crate::wayland::{CaptureSource, WaylandHelper};
use crate::{fl, with_args};

// Submodules for reorganized code
pub mod handlers;
pub mod portal;

// Re-export portal types
pub use portal::{ScreenshotOptions, ScreenshotResult};

// Re-export state types (Choice and Action are now defined in state.rs)
// NOTE: Args is still defined in this file for now, will migrate incrementally

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

    fn as_bytes(&self, _mime_type: &str) -> Option<std::borrow::Cow<'static, [u8]>> {
        Some(Cow::Owned(self.bytes.clone()))
    }
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
        _app_id: &str,
    ) -> anyhow::Result<HashMap<String, ScreenshotImage>> {
        let wayland_helper = self.wayland_helper.clone();

        let mut map = HashMap::with_capacity(outputs.len());
        for Output {
            output,
            logical_position: (_output_x, _output_y),
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

#[derive(Clone, Debug)]
pub struct Args {
    pub portal: PortalContext,
    pub capture: CaptureData,
    pub detection: DetectionState,
    pub annotations: AnnotationState,
    pub session: SessionState,
    pub ui: UiState,
}

impl Args {
    /// Clear all annotation state - eliminates duplicated clearing code
    pub fn clear_annotations(&mut self) {
        self.annotations.clear_all();
    }

    /// Clear only shape annotations (arrows, circles, rectangles) - NOT redactions
    pub fn clear_shapes(&mut self) {
        self.annotations.clear_shapes();
    }

    /// Clear OCR/QR state
    pub fn clear_ocr_qr(&mut self) {
        self.detection.clear();
    }

    /// Clear all transient state (annotations + OCR/QR)
    pub fn clear_transient_state(&mut self) {
        self.clear_annotations();
        self.clear_ocr_qr();
        self.close_all_popups();
    }

    /// Disable all drawing modes without clearing annotations
    pub fn disable_all_modes(&mut self) {
        self.annotations.disable_all_modes();
    }

    /// Close all open popups and drawers
    pub fn close_all_popups(&mut self) {
        self.ui.close_all_popups();
    }
}
struct Output {
    output: WlOutput,
    logical_position: (i32, i32),
    logical_size: (i32, i32),
    scale_factor: i32,
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
        #[zbus(connection)] _connection: &zbus::Connection,
        handle: zvariant::ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: ScreenshotOptions,
    ) -> PortalResponse<ScreenshotResult> {
        // Check if a recording is active and stop it
        if crate::screencast::is_recording() {
            log::info!("Active recording detected, stopping before showing screenshot UI");
            if let Err(e) = crate::screencast::stop_recording() {
                log::error!("Failed to stop recording: {}", e);
            }
        }

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
                scale_factor: info.scale_factor,
                name,
            });
        }
        if outputs.is_empty() {
            log::error!("No output");
            return PortalResponse::Other;
        };

        // Always interactive for blazingshot
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
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
        let config = SnapPeaConfig::load();

        // Send UI immediately with empty QR codes, detection happens async
        if let Err(err) = self
            .tx
            .send(Event::Screenshot(Args {
                portal: PortalContext {
                    handle: handle.to_owned(),
                    app_id: app_id.to_string(),
                    parent_window: parent_window.to_string(),
                    options: options.clone(),
                    tx,
                },
                capture: CaptureData {
                    output_images,
                    toplevel_images,
                },
                session: SessionState {
                    choice,
                    action: if options.choose_destination.unwrap_or_default() {
                        Action::SaveToClipboard
                    } else {
                        Action::ReturnPath
                    },
                    location: ImageSaveLocation::Pictures,
                    highlighted_window_index: 0,
                    focused_output_index: 0,
                    also_copy_to_clipboard: false,
                },
                detection: DetectionState::default(),
                annotations: AnnotationState::default(),
                ui: {
                    // Detect available encoders
                    use crate::screencast::encoder::detect_encoders;
                    let available_encoders = detect_encoders().unwrap_or_default();
                    let encoder_displays: Vec<(String, String)> = available_encoders
                        .iter()
                        .map(|e| (e.display_name(), e.gst_element.clone()))
                        .collect();

                    UiState {
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
                        tesseract_available: is_tesseract_available(),
                        available_encoders,
                        encoder_displays,
                        selected_encoder: config.video_encoder.clone(),
                        video_container: config.video_container,
                        video_framerate: config.video_framerate,
                    }
                },
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

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

pub(crate) fn view(app: &App, id: window::Id) -> cosmic::Element<'_, Msg> {
    use crate::widget::screenshot_selection::{OutputContext, ScreenshotSelectionWidget};

    let Some((i, output)) = app.outputs.iter().enumerate().find(|(_idx, o)| o.id == id) else {
        return horizontal_space().width(Length::Fixed(1.0)).into();
    };
    let Some(args) = app.screenshot_args.as_ref() else {
        return horizontal_space().width(Length::Fixed(1.0)).into();
    };
    let Some(img) = args.capture.output_images.get(&output.name) else {
        return horizontal_space().width(Length::Fixed(1.0)).into();
    };

    let theme = app.core.system_theme().cosmic();

    // Calculate derived state
    let has_any_annotations = !args.annotations.arrows.is_empty()
        || !args.annotations.circles.is_empty()
        || !args.annotations.rect_outlines.is_empty();
    let has_any_redactions =
        !args.annotations.redactions.is_empty() || !args.annotations.pixelations.is_empty();
    let has_ocr_text = args.detection.ocr_text.is_some();

    let is_active_output = {
        let output_name = &output.name;
        match &args.session.choice {
            Choice::Rectangle(_, _) => true,
            Choice::Output(None) | Choice::Window(_, None) => true,
            Choice::Window(win_output, Some(_)) => output_name == win_output,
            Choice::Output(Some(selected)) => output_name == selected,
        }
    };

    let has_confirmed_selection = matches!(
        &args.session.choice,
        Choice::Window(_, Some(_)) | Choice::Output(Some(_))
    );

    let output_ctx = OutputContext {
        output_count: app.outputs.len(),
        highlighted_window_index: args.session.highlighted_window_index,
        focused_output_index: args.session.focused_output_index,
        current_output_index: i,
        is_active_output,
        has_confirmed_selection,
    };

    // Build widget with grouped state and single event handler
    ScreenshotSelectionWidget::new(
        args.session.choice.clone(),
        img,
        &args.capture.toplevel_images,
        output,
        id,
        theme.spacing,
        i as u128,
        &args.annotations,
        &args.detection,
        &args.ui,
        output_ctx,
        has_any_annotations,
        has_any_redactions,
        has_ocr_text,
        |event| event.to_msg(),
    )
    .into()
}

pub fn update_msg(app: &mut App, msg: Msg) -> cosmic::Task<crate::core::app::Msg> {
    match msg {
        // === Draw messages - annotation drawing ===
        Msg::Draw(draw_msg) => handle_draw_msg(app, draw_msg),

        // === Tool messages - popup and tool configuration ===
        Msg::Tool(tool_msg) => handle_tool_msg(app, tool_msg),

        // === Selection messages - mode and navigation ===
        Msg::Select(select_msg) => handle_select_msg(app, select_msg),

        // === Settings messages - UI and config ===
        Msg::Settings(settings_msg) => handle_settings_msg(app, settings_msg),

        // === Detection messages - OCR and QR ===
        Msg::Detect(detect_msg) => handle_detect_msg(app, detect_msg),

        // === Capture messages - capture workflow ===
        Msg::Capture(capture_msg) => handle_capture_msg(app, capture_msg),
    }
}

/// Handle Draw messages (annotation drawing)
/// Delegates to the annotations module handler
fn handle_draw_msg(app: &mut App, msg: DrawMsg) -> cosmic::Task<crate::core::app::Msg> {
    if let Some(args) = app.screenshot_args.as_mut() {
        crate::annotations::handlers::handle_draw_msg(args, msg);
    }
    cosmic::Task::none()
}

/// Handle Tool messages (popup and tool configuration)
/// Delegates to the widget::tool_handlers module
fn handle_tool_msg(app: &mut App, msg: ToolMsg) -> cosmic::Task<crate::core::app::Msg> {
    if let Some(args) = app.screenshot_args.as_mut() {
        let needs_save = crate::widget::tool_handlers::handle_tool_msg(args, msg);
        if needs_save {
            crate::widget::tool_handlers::save_tool_config(args);
        }
    }
    cosmic::Task::none()
}

/// Handle Select messages (mode and navigation)
fn handle_select_msg(app: &mut App, msg: SelectMsg) -> cosmic::Task<crate::core::app::Msg> {
    use handlers::*;

    match msg {
        SelectMsg::RegionMode => handle_select_region_mode(app),
        SelectMsg::WindowMode(idx) => handle_select_window_mode(app, idx),
        SelectMsg::ScreenMode(idx) => handle_select_screen_mode(app, idx),
        SelectMsg::Navigate(dir) => match dir {
            Direction::Left => handle_navigate_left(app),
            Direction::Right => handle_navigate_right(app),
            Direction::Up => handle_navigate_left(app), // Same as Left
            Direction::Down => handle_navigate_right(app), // Same as Right
        },
        SelectMsg::Confirm => handle_confirm_selection(app),
    }
}

/// Handle Settings messages (UI and config)
fn handle_settings_msg(app: &mut App, msg: SettingsMsg) -> cosmic::Task<crate::core::app::Msg> {
    use crate::widget::settings_handlers;

    with_args!(app, |args| {
        match msg {
            SettingsMsg::ToolbarPosition(pos) => {
                settings_handlers::handle_toolbar_position_change(args, pos)
            }
            SettingsMsg::ToggleDrawer => settings_handlers::handle_toggle_settings_drawer(args),
            SettingsMsg::ToggleMagnifier => settings_handlers::handle_toggle_magnifier(args),
            SettingsMsg::SetSaveLocation(loc) => match loc {
                SaveLocationChoice::Pictures => {
                    settings_handlers::handle_set_save_location_pictures(args)
                }
                SaveLocationChoice::Documents => {
                    settings_handlers::handle_set_save_location_documents(args)
                }
            },
            SettingsMsg::ToggleCopyOnSave => settings_handlers::handle_toggle_copy_on_save(args),
            SettingsMsg::SetVideoEncoder(encoder) => {
                settings_handlers::handle_set_video_encoder(args, encoder)
            }
            SettingsMsg::SetVideoContainer(container) => {
                settings_handlers::handle_set_video_container(args, container)
            }
            SettingsMsg::SetVideoFramerate(framerate) => {
                settings_handlers::handle_set_video_framerate(args, framerate)
            }
        }
    })
}

/// Handle Detect messages (OCR and QR)
fn handle_detect_msg(app: &mut App, msg: DetectMsg) -> cosmic::Task<crate::core::app::Msg> {
    if let Some(args) = app.screenshot_args.as_mut() {
        if matches!(
            msg,
            DetectMsg::Qr(QrMsg::Requested)
                | DetectMsg::Ocr(OcrMsg::Requested)
                | DetectMsg::Qr(QrMsg::CopyAndClose)
                | DetectMsg::Ocr(OcrMsg::CopyAndClose)
        ) {
            args.disable_all_modes();
            args.close_all_popups();
        }
    }
    match msg {
        DetectMsg::Qr(qr_msg) => handle_qr_msg(app, qr_msg),
        DetectMsg::Ocr(ocr_msg) => handle_ocr_msg(app, ocr_msg),
    }
}

/// Handle Capture messages (capture workflow)
fn handle_capture_msg(app: &mut App, msg: CaptureMsg) -> cosmic::Task<crate::core::app::Msg> {
    match msg {
        CaptureMsg::Capture => handle_capture_inner(app),
        CaptureMsg::Cancel => handle_cancel_inner(app),
        CaptureMsg::CopyToClipboard => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.session.location = ImageSaveLocation::Clipboard;
            }
            handle_capture_inner(app)
        }
        CaptureMsg::SaveToPictures => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.session.location = match args.ui.save_location_setting {
                    SaveLocation::Pictures => ImageSaveLocation::Pictures,
                    SaveLocation::Documents => ImageSaveLocation::Documents,
                };
                if args.ui.copy_to_clipboard_on_save {
                    args.session.also_copy_to_clipboard = true;
                }
            }
            handle_capture_inner(app)
        }
        CaptureMsg::RecordRegion => {
            // Get region from selection state
            let Some(args) = app.screenshot_args.as_ref() else {
                log::warn!("Record clicked but no screenshot args available");
                return cosmic::Task::none();
            };

            let region = match &args.session.choice {
                Choice::Rectangle(rect, _) if rect.width() > 0 && rect.height() > 0 => {
                    (rect.left, rect.top, rect.width() as u32, rect.height() as u32)
                }
                Choice::Output(Some(output_name)) => {
                    // Find output dimensions
                    if let Some(output) = app.outputs.iter().find(|o| &o.name == output_name) {
                        (
                            output.logical_pos.0,
                            output.logical_pos.1,
                            output.logical_size.0,
                            output.logical_size.1,
                        )
                    } else {
                        log::warn!("Record clicked but output not found: {}", output_name);
                        return cosmic::Task::none();
                    }
                }
                Choice::Window(output_name, Some(window_index)) => {
                    // Get window dimensions from toplevel images
                    if let Some(imgs) = args.capture.toplevel_images.get(output_name) {
                        if let Some(img) = imgs.get(*window_index) {
                            if let Some(output) = app.outputs.iter().find(|o| &o.name == output_name) {
                                let orig_width = img.rgba.width() as f32;
                                let orig_height = img.rgba.height() as f32;
                                let output_width = output.logical_size.0 as f32;
                                let output_height = output.logical_size.1 as f32;

                                // Calculate display bounds (matching capture logic)
                                let max_width = output_width * 0.85;
                                let max_height = output_height * 0.85;
                                let (thumb_width, thumb_height) =
                                    if orig_width > max_width || orig_height > max_height {
                                        let pre_scale =
                                            (max_width / orig_width).min(max_height / orig_height);
                                        (orig_width * pre_scale, orig_height * pre_scale)
                                    } else {
                                        (orig_width, orig_height)
                                    };

                                let available_width = output_width - 20.0;
                                let available_height = output_height - 20.0;
                                let scale_x = available_width / thumb_width;
                                let scale_y = available_height / thumb_height;
                                let display_scale = scale_x.min(scale_y).min(1.0);

                                let display_width = thumb_width * display_scale;
                                let display_height = thumb_height * display_scale;
                                let sel_x = (output_width - display_width) / 2.0;
                                let sel_y = (output_height - display_height) / 2.0;

                                (
                                    (output.logical_pos.0 as f32 + sel_x) as i32,
                                    (output.logical_pos.1 as f32 + sel_y) as i32,
                                    display_width as u32,
                                    display_height as u32,
                                )
                            } else {
                                log::warn!("Record clicked but output not found: {}", output_name);
                                return cosmic::Task::none();
                            }
                        } else {
                            log::warn!("Record clicked but window index {} not found", window_index);
                            return cosmic::Task::none();
                        }
                    } else {
                        log::warn!("Record clicked but no windows found for output: {}", output_name);
                        return cosmic::Task::none();
                    }
                }
                _ => {
                    log::warn!("Record clicked but no valid region selected");
                    return cosmic::Task::none();
                }
            };

            // Find output with most overlap with the region
            let region_rect = crate::domain::Rect {
                left: region.0,
                top: region.1,
                right: region.0 + region.2 as i32,
                bottom: region.1 + region.3 as i32,
            };

            let selected_output = app
                .outputs
                .iter()
                .filter_map(|output| {
                    let output_rect = crate::domain::Rect {
                        left: output.logical_pos.0,
                        top: output.logical_pos.1,
                        right: output.logical_pos.0 + output.logical_size.0 as i32,
                        bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                    };

                    // Calculate overlap area
                    region_rect.intersect(output_rect).map(|intersection| {
                        let overlap_area = (intersection.right - intersection.left) as u64
                            * (intersection.bottom - intersection.top) as u64;
                        (output, overlap_area)
                    })
                })
                .max_by_key(|(_, area)| *area)
                .map(|(output, area)| {
                    log::info!(
                        "Selected output '{}' with {} pixels of overlap",
                        output.name,
                        area
                    );
                    output
                });

            let (output_name, local_region, output_logical_size) = match selected_output {
                Some(output) => {
                    // Translate global region to output-local LOGICAL coordinates
                    // The recorder will scale to physical using actual screencopy dimensions
                    let local_x = (region.0 - output.logical_pos.0).max(0);
                    let local_y = (region.1 - output.logical_pos.1).max(0);

                    // Clamp to output logical bounds
                    let clamped_w = region.2.min((output.logical_size.0 as i32 - local_x).max(0) as u32);
                    let clamped_h = region.3.min((output.logical_size.1 as i32 - local_y).max(0) as u32);

                    log::info!(
                        "Translated region: global ({}, {}, {}x{}) -> local logical ({}, {}, {}x{}) on output '{}' (logical_size={}x{})",
                        region.0, region.1, region.2, region.3,
                        local_x, local_y, clamped_w, clamped_h,
                        output.name, output.logical_size.0, output.logical_size.1
                    );

                    (
                        output.name.clone(),
                        (local_x, local_y, clamped_w, clamped_h),
                        (output.logical_size.0, output.logical_size.1),
                    )
                }
                None => {
                    // Fallback to first output with original region
                    log::warn!("No output overlap found, using first output as fallback");
                    let (output_name, logical_size) = app.outputs
                        .first()
                        .map(|o| (o.name.clone(), (o.logical_size.0, o.logical_size.1)))
                        .unwrap_or_else(|| ("Unknown".to_string(), (1920, 1080)));
                    (output_name, region, logical_size)
                }
            };

            // Generate timestamped output filename
            let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
            let config = crate::config::SnapPeaConfig::load();
            let container = config.video_container;
            let output_dir = dirs::video_dir()
                .or_else(|| dirs::home_dir())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let output_file = output_dir.join(format!("recording-{}.{}", timestamp, container.extension()));

            // Determine encoder (from config or best_encoder)
            let encoder = config
                .video_encoder
                .or_else(|| crate::screencast::best_encoder().ok().map(|e| e.gst_element))
                .unwrap_or_else(|| "x264enc".to_string());

            let framerate = config.video_framerate;

            // Spawn subprocess with --record args
            // Pass logical region and logical output size - recorder will scale to physical
            let args_vec = vec![
                "--record".to_string(),
                "--output".to_string(),
                output_file.display().to_string(),
                "--output-name".to_string(),
                output_name,
                "--region".to_string(),
                format!("{},{},{},{}", local_region.0, local_region.1, local_region.2, local_region.3),
                "--logical-size".to_string(),
                format!("{},{}", output_logical_size.0, output_logical_size.1),
                "--encoder".to_string(),
                encoder,
                "--container".to_string(),
                format!("{:?}", container),
                "--framerate".to_string(),
                framerate.to_string(),
            ];

            let exe = match std::env::current_exe() {
                Ok(exe) => exe,
                Err(e) => {
                    log::error!("Failed to get current executable: {}", e);
                    return cosmic::Task::none();
                }
            };

            match std::process::Command::new(exe).args(&args_vec).spawn() {
                Ok(child) => {
                    log::info!(
                        "Recording started: PID {}, output: {}, local_region: {:?}",
                        child.id(),
                        output_file.display(),
                        local_region
                    );
                    // Close UI - same as Cancel
                    handle_cancel_inner(app)
                }
                Err(e) => {
                    log::error!(
                        "Failed to start recording: {} (output: {}, local_region: {:?})",
                        e,
                        output_file.display(),
                        local_region
                    );
                    // TODO: Show notification to user when cosmic notification API is available
                    cosmic::Task::none()
                }
            }
        }
        CaptureMsg::Choice(c) => handle_choice_inner(app, c),
        CaptureMsg::Location(loc) => handle_location_inner(app, loc),
        CaptureMsg::OutputChanged(wl_output) => handle_output_changed_inner(app, wl_output),
        CaptureMsg::WindowChosen(name, idx) => handle_window_chosen_inner(app, name, idx),
        CaptureMsg::OpenUrl(url) => handle_open_url_inner(app, url),
    }
}

/// Handle QR detection messages
fn handle_qr_msg(app: &mut App, msg: QrMsg) -> cosmic::Task<crate::core::app::Msg> {
    match msg {
        QrMsg::Requested => handle_qr_requested_inner(app),
        QrMsg::Detected(codes) => handle_qr_detected_inner(app, codes),
        QrMsg::CopyAndClose => handle_qr_copy_and_close_inner(app),
    }
}

/// Handle OCR detection messages
fn handle_ocr_msg(app: &mut App, msg: OcrMsg) -> cosmic::Task<crate::core::app::Msg> {
    match msg {
        OcrMsg::Requested => handle_ocr_requested_inner(app),
        OcrMsg::Status(status) => handle_ocr_status_inner(app, status),
        OcrMsg::StatusClear => handle_ocr_status_clear_inner(app),
        OcrMsg::CopyAndClose => handle_ocr_copy_and_close_inner(app),
    }
}

// ============================================================================
// Inner handlers for complex capture/detection logic
// ============================================================================

fn handle_capture_inner(app: &mut App) -> cosmic::Task<crate::core::app::Msg> {
    let mut cmds: Vec<cosmic::Task<crate::core::app::Msg>> = app
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
        portal,
        capture,
        session,
        annotations: args_annotations,
        ..
    } = args;
    let tx = portal.tx;
    let choice = session.choice;
    let mut images = capture.output_images;
    let location = session.location;
    let annotations = args_annotations.annotations;
    let annotation_index = args_annotations.annotation_index;
    let also_copy_to_clipboard = session.also_copy_to_clipboard;

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
                        draw_annotations_in_order(&mut final_img, annotations, &output_rect, scale);
                    }
                }

                if let Some(ref image_path) = image_path {
                    if let Err(_err) = Screenshot::save_rgba(&final_img, image_path) {
                        log::error!("Failed to capture screenshot: {:?}", _err);
                    };
                    // Also copy to clipboard if enabled
                    if also_copy_to_clipboard {
                        let mut buffer = Vec::new();
                        if let Err(e) = Screenshot::save_rgba_to_buffer(&final_img, &mut buffer) {
                            log::error!("Failed to save screenshot to buffer: {:?}", e);
                        } else {
                            cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                        }
                    }
                } else {
                    let mut buffer = Vec::new();
                    if let Err(e) = Screenshot::save_rgba_to_buffer(&final_img, &mut buffer) {
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
        Choice::Rectangle(r, _s) => {
            if let Some(RectDimension { .. }) = r.dimensions() {
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
                        let scale_x = raw_img.rgba.width() as f32 / output.logical_size.0 as f32;
                        let scale_y = raw_img.rgba.height() as f32 / output.logical_size.1 as f32;

                        let img_x = ((intersect.left - output_rect.left) as f32 * scale_x) as u32;
                        let img_y = ((intersect.top - output_rect.top) as f32 * scale_y) as u32;
                        let img_w = (intersect.width() as f32 * scale_x) as u32;
                        let img_h = (intersect.height() as f32 * scale_y) as u32;

                        let cropped =
                            image::imageops::crop_imm(&raw_img.rgba, img_x, img_y, img_w, img_h)
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
                    draw_annotations_in_order(&mut img, annotations, &r, target_scale);
                }

                if let Some(ref image_path) = image_path {
                    if let Err(_err) = Screenshot::save_rgba(&img, image_path) {
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
                                    * target_scale) as i32,
                                bottom: ((pos.1 + output.logical_size.1 as i32) as f32
                                    * target_scale) as i32,
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
                    log::error!("No outputs available for all-screens capture");
                    success = false;
                }
            }
        }
        Choice::Window(output_name, Some(window_i)) => {
            if let Some(img) = capture
                .toplevel_images
                .get(&output_name)
                .and_then(|imgs| imgs.get(window_i))
            {
                let mut final_img = img.rgba.clone();

                // Draw annotations if any
                if !annotations.is_empty() {
                    // Find the output to calculate where the window was displayed
                    if let Some(output) = outputs.iter().find(|o| o.name == output_name) {
                        let orig_width = final_img.width() as f32;
                        let orig_height = final_img.height() as f32;
                        let output_width = output.logical_size.0 as f32;
                        let output_height = output.logical_size.1 as f32;

                        // Step 1: Calculate pre-scaled thumbnail size (matching calculate_window_display_bounds)
                        let max_width = output_width * 0.85;
                        let max_height = output_height * 0.85;
                        let (thumb_width, thumb_height) =
                            if orig_width > max_width || orig_height > max_height {
                                let pre_scale =
                                    (max_width / orig_width).min(max_height / orig_height);
                                (orig_width * pre_scale, orig_height * pre_scale)
                            } else {
                                (orig_width, orig_height)
                            };

                        // Step 2: Calculate display position (centering with 20px margin)
                        let available_width = output_width - 20.0;
                        let available_height = output_height - 20.0;
                        let scale_x = available_width / thumb_width;
                        let scale_y = available_height / thumb_height;
                        let display_scale = scale_x.min(scale_y).min(1.0);

                        let display_width = thumb_width * display_scale;
                        let display_height = thumb_height * display_scale;
                        let sel_x = (output_width - display_width) / 2.0;
                        let sel_y = (output_height - display_height) / 2.0;

                        // The selection_rect is where the window was displayed on screen (in global coords)
                        // Annotation coords are stored in global coordinates (output.left + pos.x)
                        // Image scale converts from display coords to original image pixels
                        let output_left = output.logical_pos.0 as f32;
                        let output_top = output.logical_pos.1 as f32;
                        let window_rect = Rect {
                            left: (output_left + sel_x) as i32,
                            top: (output_top + sel_y) as i32,
                            right: (output_left + sel_x + display_width) as i32,
                            bottom: (output_top + sel_y + display_height) as i32,
                        };
                        // Scale factor: original_size / display_size
                        let image_scale = orig_width / display_width;
                        draw_annotations_in_order(
                            &mut final_img,
                            annotations,
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
                        if let Err(e) = Screenshot::save_rgba_to_buffer(&final_img, &mut buffer) {
                            log::error!("Failed to save screenshot to buffer: {:?}", e);
                        } else {
                            cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                        }
                    }
                } else {
                    let mut buffer = Vec::new();
                    if let Err(e) = Screenshot::save_rgba_to_buffer(&final_img, &mut buffer) {
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
        if let Err(_err) = tx.send(response).await {
            log::error!("Failed to send screenshot event");
        }
    });
    cosmic::Task::batch(cmds)
}

fn handle_cancel_inner(app: &mut App) -> cosmic::Task<crate::core::app::Msg> {
    let cmds = app.outputs.iter().map(|o| destroy_layer_surface(o.id));
    let Some(args) = app.screenshot_args.take() else {
        log::error!("Failed to find screenshot Args for Cancel message.");
        return cosmic::Task::batch(cmds);
    };
    let Args { portal, .. } = args;
    let tx = portal.tx;
    tokio::spawn(async move {
        if let Err(_err) = tx.send(PortalResponse::Cancelled).await {
            log::error!("Failed to send screenshot event");
        }
    });

    cosmic::Task::batch(cmds)
}

fn handle_choice_inner(app: &mut App, c: Choice) -> cosmic::Task<crate::core::app::Msg> {
    if let Some(args) = app.screenshot_args.as_mut() {
        // Clear OCR/QR/arrows when rectangle changes (new selection started)
        if let Choice::Rectangle(new_r, new_s) = &c {
            if let Choice::Rectangle(old_r, _) = &args.session.choice {
                // If the rectangle position/size changed significantly, clear everything
                if new_r.left != old_r.left
                    || new_r.top != old_r.top
                    || new_r.right != old_r.right
                    || new_r.bottom != old_r.bottom
                {
                    args.clear_transient_state();
                }
            }
            // Also clear if we're starting a new drag from None state
            if *new_s != DragState::None {
                args.clear_transient_state();
            }
        }
        // Clear annotations when switching modes (Region, Window, or Output picker)
        if matches!(
            &c,
            Choice::Rectangle(_, DragState::None) | Choice::Window(_, None) | Choice::Output(None) // Only clear in picker mode, not when confirmed
        ) {
            args.clear_annotations();
            args.close_all_popups();
        }
        args.session.choice = c;
    } else {
        log::error!("Failed to find screenshot Args for Choice message.");
    }
    cosmic::Task::none()
}

fn handle_output_changed_inner(
    app: &mut App,
    wl_output: WlOutput,
) -> cosmic::Task<crate::core::app::Msg> {
    // In screen picker mode, cursor hover just updates focused_output_index
    // In confirmed mode, this is ignored (screen stays locked)
    if let Some(args) = app.screenshot_args.as_mut() {
        // Find the output index
        if let Some(output_index) = app.outputs.iter().position(|o| o.output == wl_output) {
            // Only update highlight in picker mode (None means picker)
            if matches!(args.session.choice, Choice::Output(None)) {
                args.session.focused_output_index = output_index;
            }
        }
    }
    app.active_output = Some(wl_output);
    cosmic::Task::none()
}

fn handle_window_chosen_inner(
    app: &mut App,
    name: String,
    i: usize,
) -> cosmic::Task<crate::core::app::Msg> {
    if let Some(args) = app.screenshot_args.as_mut() {
        args.session.choice = Choice::Window(name, Some(i));
        // Clear any previous state when selecting a new window
        args.clear_transient_state();
    } else {
        log::error!("Failed to find screenshot Args for WindowChosen message.");
    }
    // Don't capture immediately - let user interact with OCR/QR/arrow buttons
    cosmic::Task::none()
}

fn handle_location_inner(app: &mut App, loc: usize) -> cosmic::Task<crate::core::app::Msg> {
    if let Some(args) = app.screenshot_args.as_mut() {
        let loc = match loc {
            loc if loc == ImageSaveLocation::Clipboard as usize => ImageSaveLocation::Clipboard,
            loc if loc == ImageSaveLocation::Pictures as usize => ImageSaveLocation::Pictures,
            loc if loc == ImageSaveLocation::Documents as usize => ImageSaveLocation::Documents,
            _ => args.session.location,
        };
        args.session.location = loc;
    } else {
        log::error!("Failed to find screenshot Args for Location message.");
    }
    cosmic::Task::none()
}

fn handle_qr_requested_inner(app: &mut App) -> cosmic::Task<crate::core::app::Msg> {
    // Clear previous state and start QR scanning (keep redactions)
    if let Some(args) = app.screenshot_args.as_mut() {
        args.detection.qr_codes.clear();
        args.detection.qr_scanning = true;
        args.detection.ocr_overlays.clear();
        args.detection.ocr_status = OcrStatus::Idle;
        args.detection.ocr_text = None;
        args.clear_shapes();
        args.disable_all_modes();
        args.close_all_popups();
    }

    // Get the selection and run QR detection on that area
    if let Some(args) = app.screenshot_args.as_ref() {
        // Only use annotations up to annotation_index (respects undo)
        let annotations = args.annotations.annotations[..args.annotations.annotation_index].to_vec();
        let outputs_clone = app.outputs.clone();

        // Get image data and parameters based on choice type
        // Returns: (image, output_name, scale, origin_x, origin_y, selection_rect_for_redactions)
        let qr_params: Option<(RgbaImage, String, f32, f32, f32, Rect)> = match &args.session.choice {
            Choice::Rectangle(rect, _) if rect.width() > 0 && rect.height() > 0 => {
                let mut params = None;
                for output in &app.outputs {
                    if let Some(img) = args.capture.output_images.get(&output.name) {
                        let output_rect = Rect {
                            left: output.logical_pos.0,
                            top: output.logical_pos.1,
                            right: output.logical_pos.0 + output.logical_size.0 as i32,
                            bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                        };

                        if let Some(intersection) = rect.intersect(output_rect) {
                            let scale = img.rgba.width() as f32 / output.logical_size.0 as f32;
                            let x = ((intersection.left - output_rect.left) as f32 * scale) as u32;
                            let y = ((intersection.top - output_rect.top) as f32 * scale) as u32;
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
                args.capture.toplevel_images
                    .get(output_name)
                    .and_then(|imgs| imgs.get(*window_index))
                    .and_then(|img| {
                        // Calculate where the window was displayed (matching Capture logic)
                        outputs_clone
                            .iter()
                            .find(|o| &o.name == output_name)
                            .map(|output| {
                                let orig_width = img.rgba.width() as f32;
                                let orig_height = img.rgba.height() as f32;
                                let output_width = output.logical_size.0 as f32;
                                let output_height = output.logical_size.1 as f32;

                                // Step 1: Pre-scale to 85% of screen (matching calculate_window_display_bounds)
                                let max_width = output_width * 0.85;
                                let max_height = output_height * 0.85;
                                let (thumb_width, thumb_height) =
                                    if orig_width > max_width || orig_height > max_height {
                                        let pre_scale =
                                            (max_width / orig_width).min(max_height / orig_height);
                                        (orig_width * pre_scale, orig_height * pre_scale)
                                    } else {
                                        (orig_width, orig_height)
                                    };

                                // Step 2: Center with 20px margin
                                let available_width = output_width - 20.0;
                                let available_height = output_height - 20.0;
                                let scale_x = available_width / thumb_width;
                                let scale_y = available_height / thumb_height;
                                let display_scale = scale_x.min(scale_y).min(1.0);

                                let display_width = thumb_width * display_scale;
                                let display_height = thumb_height * display_scale;
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
                                // Scale factor: original_size / display_size
                                let img_scale = orig_width / display_width;

                                (
                                    img.rgba.clone(),
                                    output_name.clone(),
                                    img_scale,
                                    0.0,
                                    0.0,
                                    window_rect,
                                )
                            })
                    })
            }
            Choice::Output(Some(output_name)) => {
                args.capture.output_images.get(output_name).and_then(|img| {
                    outputs_clone
                        .iter()
                        .find(|o| &o.name == output_name)
                        .map(|output| {
                            let scale = img.rgba.width() as f32 / output.logical_size.0 as f32;
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
                    move |qr_codes| crate::core::app::Msg::Screenshot(Msg::qr_detected(qr_codes)),
                );
                qr_detection_tasks.push(task);
            });

            return cosmic::Task::batch(qr_detection_tasks);
        }
    }
    cosmic::Task::none()
}

fn handle_qr_detected_inner(
    app: &mut App,
    new_qr_codes: Vec<DetectedQrCode>,
) -> cosmic::Task<crate::core::app::Msg> {
    if let Some(args) = app.screenshot_args.as_mut() {
        // Scanning pass completed - hide scanning indicator after first pass
        args.detection.qr_scanning = false;

        // Merge new QR codes, avoiding duplicates
        for qr in new_qr_codes {
            if !is_duplicate_qr(&args.detection.qr_codes, &qr) {
                args.detection.qr_codes.push(qr);
            }
        }
    }
    cosmic::Task::none()
}

fn handle_ocr_requested_inner(app: &mut App) -> cosmic::Task<crate::core::app::Msg> {
    // Check if models need downloading and set appropriate status
    let needs_download = models_need_download();
    if let Some(args) = app.screenshot_args.as_mut() {
        args.detection.ocr_status = if needs_download {
            OcrStatus::DownloadingModels
        } else {
            OcrStatus::Running
        };
        // Clear previous state (keep redactions)
        args.detection.ocr_overlays.clear();
        args.detection.ocr_text = None;
        args.detection.qr_codes.clear();
        args.clear_shapes();
        args.disable_all_modes();
        args.close_all_popups();
    }

    // Get the selection and run OCR on that area
    if let Some(args) = app.screenshot_args.as_ref() {
        // Only use annotations up to annotation_index (respects undo)
        let annotations = args.annotations.annotations[..args.annotations.annotation_index].to_vec();
        let outputs_clone = app.outputs.clone();

        // Returns: (image, mapping, selection_rect_for_redactions, scale_for_redactions)
        let region_data: Option<(RgbaImage, OcrMapping, Rect, f32)> = match &args.session.choice {
            Choice::Rectangle(rect, _) if rect.width() > 0 && rect.height() > 0 => {
                // Collect image data for the selected rectangle
                let mut data = None;
                for output in &app.outputs {
                    if let Some(img) = args.capture.output_images.get(&output.name) {
                        let output_rect = Rect {
                            left: output.logical_pos.0,
                            top: output.logical_pos.1,
                            right: output.logical_pos.0 + output.logical_size.0 as i32,
                            bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                        };

                        if let Some(intersection) = rect.intersect(output_rect) {
                            let scale = img.rgba.width() as f32 / output.logical_size.0 as f32;
                            let x = ((intersection.left - output_rect.left) as f32 * scale) as u32;
                            let y = ((intersection.top - output_rect.top) as f32 * scale) as u32;
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
                args.capture.toplevel_images
                    .get(output_name)
                    .and_then(|imgs| imgs.get(*window_index))
                    .and_then(|img| {
                        // Calculate where the window was displayed (matching Capture logic)
                        outputs_clone
                            .iter()
                            .find(|o| &o.name == output_name)
                            .map(|output| {
                                let orig_width = img.rgba.width() as f32;
                                let orig_height = img.rgba.height() as f32;
                                let output_width = output.logical_size.0 as f32;
                                let output_height = output.logical_size.1 as f32;

                                // Step 1: Pre-scale to 85% of screen (matching calculate_window_display_bounds)
                                let max_width = output_width * 0.85;
                                let max_height = output_height * 0.85;
                                let (thumb_width, thumb_height) =
                                    if orig_width > max_width || orig_height > max_height {
                                        let pre_scale =
                                            (max_width / orig_width).min(max_height / orig_height);
                                        (orig_width * pre_scale, orig_height * pre_scale)
                                    } else {
                                        (orig_width, orig_height)
                                    };

                                // Step 2: Center with 20px margin
                                let available_width = output_width - 20.0;
                                let available_height = output_height - 20.0;
                                let scale_x = available_width / thumb_width;
                                let scale_y = available_height / thumb_height;
                                let display_scale = scale_x.min(scale_y).min(1.0);

                                let display_width = thumb_width * display_scale;
                                let display_height = thumb_height * display_scale;
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
                                // Scale factor: original_size / display_size
                                let img_scale = orig_width / display_width;

                                // OCR origin is where the window is displayed on the output (in output-relative coords)
                                // OCR scale converts from display coords to original image pixels
                                let ocr_scale = orig_width / display_width;

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
                            })
                    })
            }
            Choice::Output(Some(output_name)) => {
                // Get full output image
                args.capture.output_images.get(output_name).and_then(|img| {
                    outputs_clone
                        .iter()
                        .find(|o| &o.name == output_name)
                        .map(|output| {
                            let scale = img.rgba.width() as f32 / output.logical_size.0 as f32;
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
                                    size: (img.rgba.width() as f32, img.rgba.height() as f32),
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
                draw_annotations_in_order(&mut cropped_img, &annotations, &selection_rect, scale);
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
                |status| crate::core::app::Msg::Screenshot(Msg::ocr_status(status)),
            );
        }
    }
    cosmic::Task::none()
}

fn handle_ocr_status_inner(
    app: &mut App,
    status: OcrStatus,
) -> cosmic::Task<crate::core::app::Msg> {
    match &status {
        OcrStatus::Done(text, overlays) => {
            log::info!("OCR Result: {} ({} overlays)", text, overlays.len());
            if let Some(args) = app.screenshot_args.as_mut() {
                args.detection.ocr_status = status.clone();
                args.detection.ocr_overlays = overlays.clone();
                // Store text for later copying when user clicks the button
                if !text.is_empty() && text != "No text detected" {
                    args.detection.ocr_text = Some(text.clone());
                }
                log::info!("Stored {} overlays in args", args.detection.ocr_overlays.len());
            }
            // Don't auto-copy - user will click "copy text" button
        }
        OcrStatus::Error(err) => {
            log::error!("OCR Error: {}", err);
            if let Some(args) = app.screenshot_args.as_mut() {
                args.detection.ocr_status = status;
                args.detection.ocr_overlays.clear();
                args.detection.ocr_text = None;
            }
        }
        _ => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.detection.ocr_status = status;
            }
        }
    }
    cosmic::Task::none()
}

fn handle_ocr_status_clear_inner(app: &mut App) -> cosmic::Task<crate::core::app::Msg> {
    if let Some(args) = app.screenshot_args.as_mut() {
        args.detection.ocr_status = OcrStatus::Idle;
    }
    cosmic::Task::none()
}

fn handle_ocr_copy_and_close_inner(app: &mut App) -> cosmic::Task<crate::core::app::Msg> {
    // Copy OCR text and close the app
    let mut cmds: Vec<cosmic::Task<crate::core::app::Msg>> = app
        .outputs
        .iter()
        .map(|o| destroy_layer_surface(o.id))
        .collect();

    if let Some(args) = app.screenshot_args.take() {
        let tx = args.portal.tx;
        let ocr_text = args.detection.ocr_text;

        if let Some(text) = ocr_text {
            cmds.push(clipboard::write(text));
        }

        tokio::spawn(async move {
            if let Err(_err) = tx.send(PortalResponse::Cancelled).await {
                log::error!("Failed to send screenshot event");
            }
        });
    }
    cosmic::Task::batch(cmds)
}

fn handle_qr_copy_and_close_inner(app: &mut App) -> cosmic::Task<crate::core::app::Msg> {
    // Copy first QR code content and close the app
    let mut cmds: Vec<cosmic::Task<crate::core::app::Msg>> = app
        .outputs
        .iter()
        .map(|o| destroy_layer_surface(o.id))
        .collect();

    if let Some(args) = app.screenshot_args.take() {
        let tx = args.portal.tx;
        let qr_codes = args.detection.qr_codes;

        // Copy first QR code content
        if let Some(qr) = qr_codes.first() {
            cmds.push(clipboard::write(qr.content.clone()));
        }

        tokio::spawn(async move {
            if let Err(_err) = tx.send(PortalResponse::Cancelled).await {
                log::error!("Failed to send screenshot event");
            }
        });
    }
    cosmic::Task::batch(cmds)
}

fn handle_open_url_inner(app: &mut App, url: String) -> cosmic::Task<crate::core::app::Msg> {
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
    let tx = args.portal.tx;
    tokio::spawn(async move {
        if let Err(_err) = tx.send(PortalResponse::Cancelled).await {
            log::error!("Failed to send screenshot event");
        }
    });

    cosmic::Task::batch(cmds)
}

pub fn update_args(app: &mut App, args: Args) -> cosmic::Task<crate::core::app::Msg> {
    if app.outputs.len() != args.capture.output_images.len() {
        log::error!(
            "Screenshot output count mismatch: {} != {}",
            app.outputs.len(),
            args.capture.output_images.len()
        );
        log::warn!("Screenshot outputs: {:?}", app.outputs);
        log::warn!("Screenshot images: {:?}", args.capture.output_images.keys().collect::<Vec<_>>());
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
                     output, id, ..
                 }| {
                    get_layer_surface(SctkLayerSurfaceSettings {
                        id: *id,
                        layer: Layer::Overlay,
                        keyboard_interactivity: KeyboardInteractivity::Exclusive,
                        input_zone: None,
                        anchor: Anchor::all(),
                        output: IcedOutput::Output(output.clone()),
                        namespace: "snappea".to_string(),
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

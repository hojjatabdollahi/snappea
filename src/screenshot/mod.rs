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

use image::RgbaImage;
use std::borrow::Cow;
use std::{collections::HashMap, io, path::PathBuf};
use tokio::sync::mpsc::Sender;

use wayland_client::protocol::wl_output::WlOutput;
use zbus::zvariant;

use crate::capture::image::ScreenshotImage;
use crate::capture::ocr::{
    OcrMapping, OcrStatus, is_tesseract_available, models_need_download,
    run_ocr_on_image_with_status,
};
use crate::capture::qr::{DetectedQrCode, detect_qr_codes_at_resolution, is_duplicate_qr};
use crate::config::{SaveLocationChoice, SnapPeaConfig};
use crate::core::app::{App, OutputState, RecordingIndicator};
use crate::core::portal::PortalResponse;
pub use crate::domain::{Action, Choice, DragState, ImageSaveLocation, Rect, RectDimension};
use crate::render::image::draw_annotations_in_order;
use crate::session::messages::{
    CaptureMsg, DetectMsg, Direction, DrawMsg, Msg, OcrMsg, QrMsg, SelectMsg, SettingsMsg, ToolMsg,
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

    pub fn get_img_path(location: ImageSaveLocation, custom_dir: Option<&str>) -> Option<PathBuf> {
        let mut path = match (location, custom_dir) {
            // Clipboard should always return None (no file path) - check this first!
            (ImageSaveLocation::Clipboard, _) => None,
            (_, Some(custom)) if !custom.is_empty() => {
                // Use custom directory if provided and non-empty
                Some(PathBuf::from(custom))
            }
            (ImageSaveLocation::Pictures, _) => {
                dirs::picture_dir().or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
            }
            (ImageSaveLocation::Documents, _) => {
                dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
            }
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
    /// Recording was stopped via PrintScreen - clean up indicator UI
    RecordingStopped,
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
        // Check if a recording is active - if so, just stop it and return
        // The user needs to press PrintScreen again to get the screenshot UI
        if crate::screencast::is_recording() {
            log::info!(
                "Active recording detected, stopping recording (press PrintScreen again for screenshot UI)"
            );
            if let Err(e) = crate::screencast::stop_recording() {
                log::error!("Failed to stop recording: {}", e);
            }
            // Send event to clean up indicator UI
            if let Err(e) = self.tx.send(Event::RecordingStopped).await {
                log::error!("Failed to send RecordingStopped event: {}", e);
            }
            // Return cancelled - the recording was stopped, but no screenshot taken
            // User can press PrintScreen again to get the UI
            return PortalResponse::Cancelled;
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
                },
                session: SessionState {
                    choice,
                    action: if options.choose_destination.unwrap_or_default() {
                        Action::SaveToClipboard
                    } else {
                        Action::ReturnPath
                    },
                    location: ImageSaveLocation::Pictures,
                    focused_output_index: 0,
                    also_copy_to_clipboard: false,
                    has_mouse_entered: false,
                },
                detection: DetectionState::default(),
                annotations: AnnotationState::default(),
                ui: {
                    // Start with empty encoders - they will be detected asynchronously
                    // This allows the UI to show immediately without waiting for GStreamer
                    UiState {
                        toolbar_position: config.toolbar_position,
                        settings_drawer_open: false,
                        settings_tab: crate::session::state::SettingsTab::General,
                        primary_shape_tool: config.primary_shape_tool,
                        shape_popup_open: false,
                        shape_color: config.shape_color,
                        shape_shadow: config.shape_shadow,
                        primary_redact_tool: config.primary_redact_tool,
                        redact_popup_open: false,
                        pixelation_block_size: config.pixelation_block_size,
                        magnifier_enabled: config.magnifier_enabled,
                        save_location_setting: config.save_location,
                        custom_save_path: config.custom_save_path.clone(),
                        video_save_location_setting: config.video_save_location,
                        video_custom_save_path: config.video_custom_save_path.clone(),
                        copy_to_clipboard_on_save: config.copy_to_clipboard_on_save,
                        toolbar_unhovered_opacity: config.toolbar_unhovered_opacity,
                        toolbar_is_hovered: false,
                        toolbar_opacity_save_id: 0,
                        tesseract_available: is_tesseract_available(),
                        available_encoders: Vec::new(),
                        encoder_displays: Vec::new(),
                        selected_encoder: config.video_encoder.clone(),
                        video_container: config.video_container,
                        video_framerate: config.video_framerate,
                        video_show_cursor: config.video_show_cursor,
                        is_video_mode: false,
                        is_recording: false,
                        recording_annotation_mode: false,
                        pencil_popup_open: false,
                        pencil_color: config.pencil_color,
                        pencil_fade_duration: config.pencil_fade_duration,
                        pencil_thickness: config.pencil_thickness,
                        toolbar_bounds: None,
                        timeline: cosmic_time::Timeline::new(),
                        hide_toolbar_to_tray: config.hide_toolbar_to_tray,
                        move_offset: None,
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
            Choice::Output(None) => true,
            Choice::Output(Some(selected)) => output_name == selected,
        }
    };

    let has_confirmed_selection = matches!(
        &args.session.choice,
        Choice::Output(Some(_))
    );

    let output_ctx = OutputContext {
        output_count: app.outputs.len(),
        focused_output_index: args.session.focused_output_index,
        current_output_index: i,
        is_active_output,
        has_confirmed_selection,
        has_mouse_entered: args.session.has_mouse_entered,
    };

    // Build widget with grouped state and single event handler
    ScreenshotSelectionWidget::new(
        args.session.choice.clone(),
        img,
        output,
        id,
        theme.spacing,
        i as u128,
        &args.annotations,
        &args.detection,
        &args.ui,
        &app.settings_tab_model,
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
    // Handle PencilPopup actions for indicator overlay
    if let ToolMsg::PencilPopup(action) = &msg {
        if let Some(indicator) = &mut app.recording_indicator {
            match action {
                crate::session::messages::ToolPopupAction::Toggle => {
                    indicator.pencil_popup_open = !indicator.pencil_popup_open;
                    if indicator.pencil_popup_open {
                        // DISABLE annotation mode when opening popup (prevent accidental drawing)
                        indicator.annotation_mode = false;
                        if let Some(args) = app.screenshot_args.as_mut() {
                            args.ui.recording_annotation_mode = false;
                        }
                    } else {
                        // ENABLE annotation mode when closing popup (ready to draw)
                        indicator.annotation_mode = true;
                        if let Some(args) = app.screenshot_args.as_mut() {
                            args.ui.recording_annotation_mode = true;
                        }
                    }
                }
                crate::session::messages::ToolPopupAction::Open => {
                    indicator.pencil_popup_open = true;
                    // DISABLE annotation mode when opening popup
                    indicator.annotation_mode = false;
                    if let Some(args) = app.screenshot_args.as_mut() {
                        args.ui.recording_annotation_mode = false;
                    }
                }
                crate::session::messages::ToolPopupAction::Close => {
                    indicator.pencil_popup_open = false;
                    // ENABLE annotation mode when closing popup (ready to draw)
                    indicator.annotation_mode = true;
                    if let Some(args) = app.screenshot_args.as_mut() {
                        args.ui.recording_annotation_mode = true;
                    }
                }
            }

            // Update popup bounds - must match actual rendered position
            // Popup follows toolbar position and appears above or below based on available space
            if indicator.pencil_popup_open {
                let popup_width = 262.0_f32; // 230 content + 16*2 padding
                let popup_height = 380.0_f32; // Approximate height of popup content (increased to prevent overlap)
                let popup_gap = 16.0_f32; // Gap between popup and toolbar
                let toolbar_height = 72.0_f32; // Toolbar height including padding

                // Popup x follows toolbar x position
                let popup_x = indicator.toolbar_pos.0.max(0.0);

                // Determine if popup should appear above or below toolbar
                let space_above = indicator.toolbar_pos.1;
                let space_below =
                    indicator.output_size.1 - indicator.toolbar_pos.1 - toolbar_height;

                let popup_y = if space_above >= popup_height + popup_gap {
                    // Place above toolbar
                    indicator.toolbar_pos.1 - popup_gap - popup_height
                } else if space_below >= popup_height + popup_gap {
                    // Place below toolbar
                    indicator.toolbar_pos.1 + toolbar_height + popup_gap
                } else {
                    // Not enough space either way, place above and let it clip at top
                    (indicator.toolbar_pos.1 - popup_gap - popup_height).max(0.0)
                };

                indicator.pencil_popup_bounds = Some(cosmic::iced_core::Rectangle {
                    x: popup_x,
                    y: popup_y,
                    width: popup_width,
                    height: popup_height,
                });
            } else {
                indicator.pencil_popup_bounds = None;
            }

            // Recreate surface with updated input_zone
            return cosmic::Task::done(crate::core::app::Msg::ToggleAnnotationMode);
        }
    }

    // Handle ClearPencilDrawings specially since it needs access to recording_indicator
    if let ToolMsg::ClearPencilDrawings = &msg {
        if let Some(indicator) = &mut app.recording_indicator {
            indicator.annotations.clear();
            indicator.current_stroke = None;
        }
        return cosmic::Task::none();
    }

    // Handle SetPencilColor, SetPencilFadeDuration, SetPencilThickness - update recording_indicator too
    match &msg {
        ToolMsg::SetPencilColor(color) => {
            if let Some(indicator) = &mut app.recording_indicator {
                indicator.pencil_color = *color;
            }
        }
        ToolMsg::SetPencilFadeDuration(duration) => {
            if let Some(indicator) = &mut app.recording_indicator {
                indicator.pencil_fade_duration = *duration;
            }
        }
        ToolMsg::SetPencilThickness(thickness) => {
            if let Some(indicator) = &mut app.recording_indicator {
                indicator.pencil_thickness = *thickness;
            }
        }
        _ => {}
    }

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
    use crate::session::state::SettingsTab;
    use crate::widget::settings_handlers;

    // Handle SettingsTabActivated specially - we need access to app.settings_tab_model
    if let SettingsMsg::SettingsTabActivated(entity) = msg {
        // Look up the SettingsTab data from the entity
        let tab = app
            .settings_tab_model
            .data::<SettingsTab>(entity)
            .copied()
            .unwrap_or(SettingsTab::General);

        // Activate the entity in the model
        app.settings_tab_model.activate(entity);

        // Update args.ui.settings_tab
        if let Some(args) = app.screenshot_args.as_mut() {
            args.ui.settings_tab = tab;
        }
        return cosmic::Task::none();
    }

    // Handle ToggleDrawer specially - we need to sync tab state when opening
    if let SettingsMsg::ToggleDrawer = msg {
        if let Some(args) = app.screenshot_args.as_mut() {
            args.ui.settings_drawer_open = !args.ui.settings_drawer_open;

            // When opening the drawer, sync the tab state with the model
            if args.ui.settings_drawer_open {
                // Get the currently active tab from the model using active_data
                if let Some(&tab) = app.settings_tab_model.active_data::<SettingsTab>() {
                    args.ui.settings_tab = tab;
                }
                // Close other popups
                args.ui.shape_popup_open = false;
                args.ui.redact_popup_open = false;
                args.disable_all_modes();
            }
        }
        return cosmic::Task::none();
    }

    // Handle BrowseSaveLocation specially - need to hide overlay, open dialog, then restore
    if let SettingsMsg::BrowseSaveLocation = msg {
        // Destroy layer surfaces (but keep screenshot_args intact)
        let destroy_tasks: Vec<_> = app
            .outputs
            .iter()
            .map(|o| destroy_layer_surface(o.id))
            .collect();

        // Open file dialog and send result
        let dialog_task = cosmic::Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title("Select save location for screenshots")
                    .pick_folder()
                    .await
                    .map(|handle| handle.path().to_string_lossy().to_string())
            },
            |result| {
                crate::core::app::Msg::Screenshot(crate::session::messages::Msg::browse_save_location_result(result))
            },
        );

        return cosmic::Task::batch(destroy_tasks).chain(dialog_task);
    }

    // Handle BrowseSaveLocationResult - restore overlay and optionally set path
    if let SettingsMsg::BrowseSaveLocationResult(path) = msg {
        // If a path was selected, update settings
        if let Some(path) = path {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.ui.custom_save_path = path.clone();
                let mut config = SnapPeaConfig::load();
                config.custom_save_path = path;
                config.save();
            }
        }

        // Recreate layer surfaces
        return recreate_screenshot_surfaces(app);
    }

    // Handle BrowseVideoSaveLocation specially - same pattern
    if let SettingsMsg::BrowseVideoSaveLocation = msg {
        let destroy_tasks: Vec<_> = app
            .outputs
            .iter()
            .map(|o| destroy_layer_surface(o.id))
            .collect();

        let dialog_task = cosmic::Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title("Select save location for videos")
                    .pick_folder()
                    .await
                    .map(|handle| handle.path().to_string_lossy().to_string())
            },
            |result| {
                crate::core::app::Msg::Screenshot(crate::session::messages::Msg::browse_video_save_location_result(result))
            },
        );

        return cosmic::Task::batch(destroy_tasks).chain(dialog_task);
    }

    // Handle BrowseVideoSaveLocationResult - restore overlay and optionally set path
    if let SettingsMsg::BrowseVideoSaveLocationResult(path) = msg {
        if let Some(path) = path {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.ui.video_custom_save_path = path.clone();
                let mut config = SnapPeaConfig::load();
                config.video_custom_save_path = path;
                config.save();
            }
        }

        return recreate_screenshot_surfaces(app);
    }

    with_args!(app, |args| {
        match msg {
            SettingsMsg::ToolbarPosition(pos) => {
                settings_handlers::handle_toolbar_position_change(args, pos)
            }
            SettingsMsg::ToggleDrawer => {
                // Already handled above
                cosmic::Task::none()
            }
            SettingsMsg::ToggleMagnifier => settings_handlers::handle_toggle_magnifier(args),
            SettingsMsg::SetSaveLocation(loc) => match loc {
                SaveLocationChoice::Pictures => {
                    settings_handlers::handle_set_save_location_pictures(args)
                }
                SaveLocationChoice::Documents => {
                    settings_handlers::handle_set_save_location_documents(args)
                }
                SaveLocationChoice::Custom => {
                    settings_handlers::handle_set_save_location_custom(args)
                }
            },
            SettingsMsg::SetCustomSavePath(path) => {
                settings_handlers::handle_set_custom_save_path(args, path)
            }
            SettingsMsg::BrowseSaveLocation => {
                // Already handled above (special handling for overlay hide/restore)
                cosmic::Task::none()
            }
            SettingsMsg::BrowseSaveLocationResult(_) => {
                // Already handled above
                cosmic::Task::none()
            }
            SettingsMsg::SetVideoSaveLocation(loc) => {
                settings_handlers::handle_set_video_save_location(args, loc)
            }
            SettingsMsg::SetVideoCustomSavePath(path) => {
                settings_handlers::handle_set_video_custom_save_path(args, path)
            }
            SettingsMsg::BrowseVideoSaveLocation => {
                // Already handled above (special handling for overlay hide/restore)
                cosmic::Task::none()
            }
            SettingsMsg::BrowseVideoSaveLocationResult(_) => {
                // Already handled above
                cosmic::Task::none()
            }
            SettingsMsg::ToggleCopyOnSave => settings_handlers::handle_toggle_copy_on_save(args),
            SettingsMsg::SettingsTabActivated(_) => {
                // Already handled above
                cosmic::Task::none()
            }
            SettingsMsg::SetToolbarOpacity(opacity) => {
                settings_handlers::handle_set_toolbar_opacity(args, opacity)
            }
            SettingsMsg::SaveToolbarOpacityDebounced(opacity, save_id) => {
                settings_handlers::handle_save_toolbar_opacity_debounced(args, opacity, save_id)
            }
            SettingsMsg::ToolbarHoverChanged(is_hovered) => {
                args.ui.toolbar_is_hovered = is_hovered;
                // Start the appropriate animation
                if is_hovered {
                    args.ui
                        .timeline
                        .set_chain(crate::widget::toolbar::toolbar_fade_in())
                        .start();
                } else {
                    args.ui
                        .timeline
                        .set_chain(crate::widget::toolbar::toolbar_fade_out())
                        .start();
                }
                cosmic::Task::none()
            }
            SettingsMsg::ToolbarBounds(bounds) => {
                args.ui.toolbar_bounds = Some(bounds);
                if let Some(indicator) = &mut app.recording_indicator {
                    indicator.toolbar_bounds = Some(bounds);
                }
                cosmic::Task::none()
            }
            SettingsMsg::SetVideoEncoder(encoder) => {
                settings_handlers::handle_set_video_encoder(args, encoder)
            }
            SettingsMsg::SetVideoContainer(container) => {
                settings_handlers::handle_set_video_container(args, container)
            }
            SettingsMsg::SetVideoFramerate(framerate) => {
                settings_handlers::handle_set_video_framerate(args, framerate)
            }
            SettingsMsg::ToggleShowCursor => {
                args.ui.video_show_cursor = !args.ui.video_show_cursor;
                // Persist to config
                let mut config = crate::config::SnapPeaConfig::load();
                config.video_show_cursor = args.ui.video_show_cursor;
                config.save();
                cosmic::Task::none()
            }
            SettingsMsg::ToggleHideToTray => {
                args.ui.hide_toolbar_to_tray = !args.ui.hide_toolbar_to_tray;
                // Persist to config
                let mut config = crate::config::SnapPeaConfig::load();
                config.hide_toolbar_to_tray = args.ui.hide_toolbar_to_tray;
                config.save();
                cosmic::Task::none()
            }
            SettingsMsg::EncodersDetected(encoders) => {
                // Update UI with detected encoders
                let encoder_displays: Vec<(String, String)> = encoders
                    .iter()
                    .map(|e| (e.display_name(), e.gst_element.clone()))
                    .collect();
                log::debug!("Encoders detected: {} available", encoders.len());
                args.ui.available_encoders = encoders;
                args.ui.encoder_displays = encoder_displays;
                cosmic::Task::none()
            }
            SettingsMsg::TimelineTick(_window_id, instant) => {
                // Update the timeline's current time for animation interpolation
                args.ui.timeline.now(instant);
                cosmic::Task::none()
            }
            SettingsMsg::SetMoveOffset(offset) => {
                args.ui.move_offset = offset;
                cosmic::Task::none()
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
                    SaveLocationChoice::Pictures => ImageSaveLocation::Pictures,
                    SaveLocationChoice::Documents => ImageSaveLocation::Documents,
                    SaveLocationChoice::Custom => {
                        // For custom save location, we use Pictures as the base
                        // but the actual path is handled in save_screenshot
                        ImageSaveLocation::Pictures
                    }
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
                Choice::Rectangle(rect, _) if rect.width() > 0 && rect.height() > 0 => (
                    rect.left,
                    rect.top,
                    rect.width() as u32,
                    rect.height() as u32,
                ),
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
                    let clamped_w = region
                        .2
                        .min((output.logical_size.0 as i32 - local_x).max(0) as u32);
                    let clamped_h = region
                        .3
                        .min((output.logical_size.1 as i32 - local_y).max(0) as u32);

                    log::info!(
                        "Translated region: global ({}, {}, {}x{}) -> local logical ({}, {}, {}x{}) on output '{}' (logical_size={}x{})",
                        region.0,
                        region.1,
                        region.2,
                        region.3,
                        local_x,
                        local_y,
                        clamped_w,
                        clamped_h,
                        output.name,
                        output.logical_size.0,
                        output.logical_size.1
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
                    let (output_name, logical_size) = app
                        .outputs
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

            // Ensure output directory exists
            if !output_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(&output_dir) {
                    log::error!(
                        "Failed to create output directory '{}': {}",
                        output_dir.display(),
                        e
                    );
                    return cosmic::Task::none();
                }
            }

            let output_file =
                output_dir.join(format!("recording-{}.{}", timestamp, container.extension()));

            // Determine encoder (from config or best_encoder)
            let encoder = config
                .video_encoder
                .or_else(|| {
                    crate::screencast::best_encoder()
                        .ok()
                        .map(|e| e.gst_element)
                })
                .unwrap_or_else(|| "x264enc".to_string());

            let framerate = config.video_framerate;
            let show_cursor = args.ui.video_show_cursor;
            log::info!("Cursor visibility setting: {}", show_cursor);

            // Build CaptureSource for thread-based recording
            // IMPORTANT: We must look up objects in app.wayland_helper's connection,
            // NOT use the WlOutput/toplevel objects from cosmic-iced's connection
            // (they are different Wayland connections with different object IDs)
            let output_name_for_indicator = output_name.clone();
            // Use output capture with region cropping
            // Look up output by NAME in wayland_helper's connection
            let wl_output = app.wayland_helper.outputs().into_iter().find(|o| {
                app.wayland_helper
                    .output_info(o)
                    .and_then(|info| info.name)
                    .as_deref()
                    == Some(&output_name)
            });

            let capture_source = match wl_output {
                Some(output) => {
                    log::info!(
                        "Recording output '{}' region: {:?}",
                        output_name,
                        local_region
                    );
                    CaptureSource::Output(output)
                }
                None => {
                    log::error!("Output '{}' not found in wayland_helper", output_name);
                    return cosmic::Task::none();
                }
            };

            // Clone wayland_helper for the recording thread
            let wayland_helper = app.wayland_helper.clone();

            // Create stop flag for graceful shutdown
            let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stop_flag_clone = stop_flag.clone();

            // Create recording state
            let recording_state = crate::screencast::RecordingState {
                output_file: output_file.clone(),
                region: local_region,
                output_name: output_name.clone(),
                started_at: chrono::Utc::now().to_rfc3339(),
            };

            // Save recording state to disk for external tools
            if let Err(e) = recording_state.save() {
                log::warn!("Failed to save recording state: {}", e);
            }

            // Spawn recording thread
            let output_file_for_thread = output_file.clone();
            let encoder_clone = encoder.clone();
            let thread_handle = std::thread::spawn(move || {
                crate::screencast::start_recording_thread(
                    wayland_helper,
                    capture_source,
                    output_file_for_thread,
                    local_region,
                    output_logical_size,
                    encoder_clone,
                    container,
                    framerate,
                    show_cursor,
                    stop_flag_clone,
                )
            });

            // Register the recording handle
            let recording_handle =
                crate::screencast::RecordingHandle::new(stop_flag, thread_handle, recording_state);
            crate::screencast::set_recording(recording_handle);

            log::info!(
                "Recording started: output: {}, local_region: {:?}",
                output_file.display(),
                local_region
            );

            // Set recording mode in UI state
            if let Some(args) = app.screenshot_args.as_mut() {
                args.ui.is_recording = true;
                args.ui.recording_annotation_mode = false;
                // Close any open popups/drawers
                args.close_all_popups();
            }

            // Close screenshot UI windows - toolbar will be shown on indicator overlay
            let close_tasks: Vec<_> = app
                .outputs
                .iter()
                .map(|output| destroy_layer_surface(output.id))
                .collect();

            // Calculate toolbar position and bounds for the indicator
            let output_size = selected_output
                .map(|o| (o.logical_size.0 as f32, o.logical_size.1 as f32))
                .unwrap_or((1920.0, 1080.0));

            // Toolbar dimensions (must match rendering)
            let toolbar_width = 280.0_f32;
            let toolbar_height = 56.0_f32;
            let toolbar_margin = 32.0_f32;

            // Bottom center position
            let toolbar_x = (output_size.0 - toolbar_width) / 2.0;
            let toolbar_y = output_size.1 - toolbar_height - toolbar_margin;

            let toolbar_input_bounds = cosmic::iced_core::Rectangle {
                x: toolbar_x,
                y: toolbar_y,
                width: toolbar_width,
                height: toolbar_height,
            };

            // Create recording indicator overlay (shows red border + toolbar)
            let indicator_id = window::Id::unique();
            let indicator_output = selected_output
                .map(|o| o.output.clone())
                .or_else(|| app.outputs.first().map(|o| o.output.clone()));

            let indicator_task = if let Some(wl_output) = indicator_output {
                // Store the indicator state WITH toolbar
                app.recording_indicator = Some(RecordingIndicator {
                    window_id: indicator_id,
                    output_name: output_name_for_indicator,
                    output: wl_output.clone(),
                    output_size,
                    region: local_region,
                    blink_visible: true,
                    annotations: Vec::new(),
                    current_stroke: None,
                    super_pressed: false,
                    ctrl_pressed: false,
                    annotation_mode: false,
                    pencil_color: config.pencil_color,
                    pencil_fade_duration: config.pencil_fade_duration,
                    pencil_thickness: config.pencil_thickness,
                    toolbar_bounds: Some(toolbar_input_bounds),
                    toolbar_pos: (toolbar_x, toolbar_y),
                    toolbar_dragging: false,
                    drag_offset: (0.0, 0.0),
                    pencil_popup_open: false,
                    pencil_popup_bounds: None,
                });

                log::info!(
                    "Creating recording indicator with toolbar: id={:?}, region={:?}, toolbar={:?}",
                    indicator_id,
                    local_region,
                    toolbar_input_bounds
                );

                // Create layer surface for the indicator
                // ONLY captures input in toolbar area - everything else clicks through!
                get_layer_surface(SctkLayerSurfaceSettings {
                    id: indicator_id,
                    layer: Layer::Overlay,
                    keyboard_interactivity: KeyboardInteractivity::OnDemand, // For Escape key
                    input_zone: Some(vec![toolbar_input_bounds]), // ONLY capture toolbar!
                    anchor: Anchor::all(),
                    output: IcedOutput::Output(wl_output),
                    namespace: "snappea-indicator".to_string(),
                    size: Some((None, None)),
                    exclusive_zone: -1,
                    size_limits: Limits::NONE.min_height(1.0).min_width(1.0),
                    ..Default::default()
                })
            } else {
                cosmic::Task::none()
            };

            // If hide_toolbar_to_tray is enabled, spawn tray and hide toolbar
            // Always create tray icon when recording
            if let Some(ref tx) = app.tray_tx {
                // toolbar_visible parameter reflects initial toolbar state
                let toolbar_visible = !config.hide_toolbar_to_tray;
                log::info!(
                    "Creating system tray for recording (toolbar_visible={})",
                    toolbar_visible
                );
                let tray_handle = crate::tray::create_tray(toolbar_visible, tx.clone());
                app.tray_handle = Some(tray_handle);
                app.toolbar_visible = toolbar_visible;
            } else {
                log::warn!("Tray sender not available");
                app.toolbar_visible = true;
            }

            // Close screenshot UI and show indicator with toolbar
            cosmic::Task::batch(close_tasks).chain(indicator_task)
        }
        CaptureMsg::Choice(c) => handle_choice_inner(app, c),
        CaptureMsg::Location(loc) => handle_location_inner(app, loc),
        CaptureMsg::OutputChanged(wl_output) => handle_output_changed_inner(app, wl_output),
        CaptureMsg::OpenUrl(url) => handle_open_url_inner(app, url),
        CaptureMsg::ToggleCaptureMode(is_video) => {
            log::info!("Capture mode toggled: is_video_mode = {}", is_video);
            if let Some(args) = app.screenshot_args.as_mut() {
                args.ui.is_video_mode = is_video;
                // Reset UI state completely when switching modes
                args.close_all_popups();
                args.disable_all_modes();
                args.clear_annotations();
                // Reset selection to empty rectangle (no selection)
                args.session.choice = Choice::Rectangle(Rect::default(), DragState::default());
                // Set up the animation chain
                let chain = if is_video {
                    crate::widget::icon_toggle::toggle_to_video()
                } else {
                    crate::widget::icon_toggle::toggle_to_screenshot()
                };
                args.ui.timeline.set_chain(chain).start();
            }
            cosmic::Task::none()
        }
        CaptureMsg::PencilRightClick => {
            // If popup is open, close it (and enable pencil). Otherwise open popup.
            let action = if app
                .recording_indicator
                .as_ref()
                .map_or(false, |i| i.pencil_popup_open)
            {
                crate::session::messages::ToolPopupAction::Close
            } else {
                crate::session::messages::ToolPopupAction::Open
            };
            cosmic::Task::done(crate::core::app::Msg::Screenshot(
                crate::session::messages::Msg::Tool(
                    crate::session::messages::ToolMsg::PencilPopup(action),
                ),
            ))
        }
        CaptureMsg::HideToTray => {
            log::info!("Hide to tray requested");
            // Update tray to show toolbar is hidden
            if let Some(ref handle) = app.tray_handle {
                handle.update(|tray| {
                    tray.set_toolbar_visible(false);
                });
            }
            app.toolbar_visible = false;

            // Disable pencil/annotation mode when hiding to tray
            if let Some(indicator) = app.recording_indicator.as_mut() {
                indicator.annotation_mode = false;
                indicator.pencil_popup_open = false;
                indicator.pencil_popup_bounds = None;
            }

            // Recreate the indicator surface without toolbar input zone
            if app.recording_indicator.is_some() {
                return cosmic::Task::done(crate::core::app::Msg::ToggleAnnotationMode);
            }
            cosmic::Task::none()
        }
        CaptureMsg::StopRecording => {
            log::info!("Stop recording requested");
            // Stop the recording
            if let Err(e) = crate::screencast::stop_recording() {
                log::error!("Failed to stop recording: {}", e);
            }

            // Clean up tray if active
            if let Some(handle) = app.tray_handle.take() {
                handle.shutdown();
            }
            app.toolbar_visible = true;

            // Destroy indicator overlay
            let indicator_task = if let Some(indicator) = app.recording_indicator.take() {
                destroy_layer_surface(indicator.window_id)
            } else {
                cosmic::Task::none()
            };

            // Clear screenshot session completely
            // Screenshot windows were already destroyed when recording started
            // Just clean up state so next PrintScreen works
            if let Some(args) = app.screenshot_args.take() {
                let tx = args.portal.tx;
                tokio::spawn(async move {
                    if let Err(_err) = tx.send(PortalResponse::Cancelled).await {
                        log::error!("Failed to send screenshot event");
                    }
                });
            }

            // Note: Don't clear app.outputs - those are Wayland outputs that remain valid
            // The window IDs in outputs are stale (windows were destroyed), but they'll be
            // recreated with new IDs when the next screenshot session starts

            indicator_task
        }
        CaptureMsg::ToggleRecordingAnnotation => {
            // If popup is open, close it (and enable pencil)
            if let Some(indicator) = app.recording_indicator.as_ref() {
                if indicator.pencil_popup_open {
                    log::info!("Pencil clicked while popup open - closing popup");
                    return cosmic::Task::done(crate::core::app::Msg::Screenshot(
                        crate::session::messages::Msg::Tool(
                            crate::session::messages::ToolMsg::PencilPopup(
                                crate::session::messages::ToolPopupAction::Close,
                            ),
                        ),
                    ));
                }
            }

            // Normal toggle behavior
            if let Some(indicator) = app.recording_indicator.as_mut() {
                indicator.annotation_mode = !indicator.annotation_mode;
                log::info!("Toggle annotation mode: {}", indicator.annotation_mode);
            }
            if let Some(args) = app.screenshot_args.as_mut() {
                args.ui.recording_annotation_mode = !args.ui.recording_annotation_mode;
            }
            // Return a task that sends ToggleAnnotationMode to properly
            // recreate the layer surface with correct input zone
            cosmic::Task::done(crate::core::app::Msg::ToggleAnnotationMode)
        }
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
        ui,
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

    // Determine custom save path based on save location setting
    let custom_dir = match ui.save_location_setting {
        SaveLocationChoice::Custom if !ui.custom_save_path.is_empty() => {
            Some(ui.custom_save_path.as_str())
        }
        _ => None,
    };

    let mut success = true;
    let image_path = Screenshot::get_img_path(location, custom_dir);

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
    // Stop recording if active
    if crate::screencast::is_recording() {
        log::info!("Canceling - stopping active recording");
        if let Err(e) = crate::screencast::stop_recording() {
            log::error!("Failed to stop recording: {}", e);
        }
    }

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
            Choice::Rectangle(_, DragState::None) | Choice::Output(None) // Only clear in picker mode, not when confirmed
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
            // Mark that mouse has entered an output (for initial highlight)
            args.session.has_mouse_entered = true;
            // Update focused_output_index to the output where mouse entered
            args.session.focused_output_index = output_index;
        }
    }
    app.active_output = Some(wl_output);
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
        let annotations =
            args.annotations.annotations[..args.annotations.annotation_index].to_vec();
        let outputs_clone = app.outputs.clone();

        // Get image data and parameters based on choice type
        // Returns: (image, output_name, scale, origin_x, origin_y, selection_rect_for_redactions)
        let qr_params: Option<(RgbaImage, String, f32, f32, f32, Rect)> = match &args.session.choice
        {
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
        let annotations =
            args.annotations.annotations[..args.annotations.annotation_index].to_vec();
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
                log::info!(
                    "Stored {} overlays in args",
                    args.detection.ocr_overlays.len()
                );
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
        log::warn!(
            "Screenshot images: {:?}",
            args.capture.output_images.keys().collect::<Vec<_>>()
        );
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

    // Check if we need to create windows:
    // - No previous args: create windows
    // - Previous args existed but was recording (windows were destroyed): create windows
    // - Previous args existed and windows are active: just update args
    let old_args = app.screenshot_args.replace(args);
    let need_windows = match &old_args {
        None => true,
        Some(old) => {
            // If old session was recording, windows were destroyed so we need to recreate
            old.ui.is_recording
        }
    };

    // Clean up old portal response channel if we're replacing args
    if let Some(old) = old_args {
        let tx = old.portal.tx;
        tokio::spawn(async move {
            // Send cancelled to the old portal request so D-Bus doesn't hang
            let _ = tx.send(PortalResponse::Cancelled).await;
        });
    }

    // Clean up recording indicator if it's still active
    // (This can happen if recording was stopped via D-Bus call rather than UI)
    let indicator_cleanup = if let Some(indicator) = app.recording_indicator.take() {
        // Also clean up tray
        if let Some(handle) = app.tray_handle.take() {
            handle.shutdown();
        }
        app.toolbar_visible = true;
        destroy_layer_surface(indicator.window_id)
    } else {
        cosmic::Task::none()
    };

    if need_windows {
        log::info!("Creating new screenshot windows");
        // Generate fresh window IDs for this session
        for output in &mut app.outputs {
            output.id = window::Id::unique();
        }

        let cmds: Vec<_> = app
            .outputs
            .iter()
            .map(|OutputState { output, id, .. }| {
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
            })
            .collect();

        // Detect encoders asynchronously so UI appears immediately
        let encoder_task = cosmic::Task::perform(
            async {
                // Run encoder detection in a blocking task to not block the async runtime
                tokio::task::spawn_blocking(|| {
                    use crate::screencast::encoder::detect_encoders;
                    detect_encoders().unwrap_or_default()
                })
                .await
                .unwrap_or_default()
            },
            |encoders| {
                crate::core::app::Msg::Screenshot(crate::session::messages::Msg::Settings(
                    crate::session::messages::SettingsMsg::EncodersDetected(encoders),
                ))
            },
        );

        indicator_cleanup
            .chain(cosmic::Task::batch(cmds))
            .chain(encoder_task)
    } else {
        log::info!("Existing screenshot args updated (windows already exist)");
        indicator_cleanup
    }
}

/// Recreate screenshot layer surfaces after they were temporarily destroyed
/// (e.g., to show a file dialog).
/// This generates new window IDs and creates new layer surfaces.
fn recreate_screenshot_surfaces(app: &mut App) -> cosmic::Task<crate::core::app::Msg> {
    // Generate fresh window IDs for this session
    for output in &mut app.outputs {
        output.id = window::Id::unique();
    }

    let cmds: Vec<_> = app
        .outputs
        .iter()
        .map(|OutputState { output, id, .. }| {
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
        })
        .collect();

    cosmic::Task::batch(cmds)
}

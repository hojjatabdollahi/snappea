// SPDX-License-Identifier: GPL-3.0-only

//! Starting a capture, and saving or discarding its result.

use super::Capture;
use crate::app::{App, OutputState};
use crate::config::{Config, SaveLocationChoice};
use crate::dbus::PortalResponse;
use crate::dbus::ScreenshotResult;
use crate::fl;
use crate::geometry::{Choice, DragState, Rect};
use cosmic::iced::clipboard::mime::AsMimeTypes;
use cosmic::iced::platform_specific::shell::commands::layer_surface::destroy_layer_surface;
use cosmic::iced::runtime::clipboard;
use cosmic::iced::window;
use image::DynamicImage;
use image::RgbaImage;
use std::borrow::Cow;
use std::io;
use std::path::PathBuf;
use tokio::sync::mpsc::Sender;
use viewer_tools::ToolOperation;

struct ScreenshotBytes {
    bytes: Vec<u8>,
}

impl AsMimeTypes for ScreenshotBytes {
    fn available(&self) -> std::borrow::Cow<'static, [String]> {
        Cow::Owned(vec!["image/png".to_string()])
    }

    fn as_bytes(&self, _mime_type: &str) -> Option<std::borrow::Cow<'static, [u8]>> {
        Some(Cow::Owned(self.bytes.clone()))
    }
}

pub fn save_rgba(img: &RgbaImage, path: &PathBuf) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(path)?;
    Ok(write_png(&mut file, img)?)
}

pub fn save_rgba_to_buffer(img: &RgbaImage, buffer: &mut Vec<u8>) -> anyhow::Result<()> {
    Ok(write_png(buffer, img)?)
}

/// A fresh file path in the chosen folder, or `None` for the clipboard. An
/// empty custom folder falls back to Pictures.
pub fn get_img_path(location: SaveLocationChoice, custom_dir: &str) -> Option<PathBuf> {
    let dir = match location {
        SaveLocationChoice::Clipboard => return None,
        SaveLocationChoice::Custom if !custom_dir.is_empty() => PathBuf::from(custom_dir),
        // An empty custom folder falls back to the default
        other => other.dir().unwrap_or_else(crate::config::screenshots_dir),
    };
    // The screenshots folder need not exist yet
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(img_file_name()))
}

/// Timestamped file name for a new screenshot.
pub fn img_file_name() -> String {
    chrono::Local::now()
        .format(&format!(
            "{}_%Y-%m-%d_%H-%M-%S.png",
            fl!("screenshot-filename-prefix")
        ))
        .to_string()
}

/// Stitch whole-output frames, each with its logical rect, into one image at
/// the first frame's scale.
pub fn stitch_outputs(frames: Vec<(RgbaImage, Rect)>) -> Option<RgbaImage> {
    let (first, first_rect) = frames.first()?;
    let scale = first.width() as f32 / first_rect.width() as f32;
    let bounds = frames.iter().map(|(_, r)| *r).reduce(Rect::union)?;
    let frames = frames
        .into_iter()
        .map(|(img, r)| (img, r.scaled(scale)))
        .collect();
    Some(combined_image(bounds.scaled(scale), frames))
}

/// The part of `img`, which shows `output` whole, under `region`: the crop,
/// the part of `region` it shows, and the crop's pixels per logical unit.
pub fn crop_to_output(
    img: &RgbaImage,
    output: &OutputState,
    region: Rect,
) -> Option<(RgbaImage, Rect, f32)> {
    let output_rect = output.rect();
    let visible = region.intersect(output_rect)?;
    let scale_x = img.width() as f32 / output.logical_size.0 as f32;
    let scale_y = img.height() as f32 / output.logical_size.1 as f32;
    let x = ((visible.left - output_rect.left) as f32 * scale_x) as u32;
    let y = ((visible.top - output_rect.top) as f32 * scale_y) as u32;
    let w = (visible.width() as f32 * scale_x) as u32;
    let h = (visible.height() as f32 * scale_y) as u32;
    let cropped = image::imageops::crop_imm(img, x, y, w, h).to_image();
    Some((cropped, visible, scale_x))
}

/// Burn `ops` into `img`, which shows `region` of the desktop at `scale`
/// pixels per logical unit.
pub fn rasterize(img: &mut RgbaImage, ops: &[Box<dyn ToolOperation>], region: &Rect, scale: f32) {
    if ops.is_empty() {
        return;
    }
    let mut image = DynamicImage::ImageRgba8(std::mem::take(img));
    viewer_tools::apply_all(
        ops,
        &mut image,
        cosmic::iced::Rectangle::new(
            cosmic::iced::Point::new(region.left as f32, region.top as f32),
            cosmic::iced::Size::new(region.width() as f32, region.height() as f32),
        ),
        scale,
    );
    *img = image.into_rgba8();
}

pub fn combined_image(bounds: Rect, frames: Vec<(RgbaImage, Rect)>) -> RgbaImage {
    if frames.len() == 1 {
        let (frame_image, rect) = &frames[0];

        let width_scale = f64::from(frame_image.width()) / f64::from(rect.width());
        let height_scale = f64::from(frame_image.height()) / f64::from(rect.height());

        let width = (f64::from(bounds.width()) * width_scale).max(0.) as u32;
        let height = (f64::from(bounds.height()) * height_scale).max(0.) as u32;
        let x = (f64::from(bounds.left - rect.left) * width_scale).max(0.) as u32;
        let y = (f64::from(bounds.top - rect.top) * height_scale).max(0.) as u32;

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
        }
        let x = i64::from(rect.left) - i64::from(bounds.left);
        let y = i64::from(rect.top) - i64::from(bounds.top);
        image::imageops::overlay(&mut image, &frame_image, x, y);
    }
    image
}

pub fn write_png<W: io::Write>(w: W, image: &RgbaImage) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(w, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // Fast compression: the default zlib level dominates save latency for a modest size gain.
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(image.as_raw())
}

/// Deliver the portal response if a caller is waiting. User-initiated captures
/// drop the receiver at once, so a failed send there is normal (issue #17).
pub fn send_portal_response(
    tx: Sender<PortalResponse<ScreenshotResult>>,
    expects_response: bool,
    response: PortalResponse<ScreenshotResult>,
) {
    tokio::spawn(async move {
        if tx.send(response).await.is_err() {
            if expects_response {
                log::error!("Portal caller is no longer waiting for the screenshot response");
            } else {
                log::debug!(
                    "No portal caller awaiting the response (user-initiated capture); ignoring"
                );
            }
        }
    });
}

/// Write `img` to `path` if there is one, and to the clipboard when asked or
/// when there is no file. Returns whether every destination took it.
fn deliver(
    img: &RgbaImage,
    path: Option<&PathBuf>,
    also_clipboard: bool,
    cmds: &mut Vec<cosmic::Task<crate::app::Msg>>,
) -> bool {
    let mut ok = true;
    if let Some(path) = path
        && let Err(err) = save_rgba(img, path)
    {
        log::error!("Failed to save screenshot: {err:?}");
        // Must flag failure, otherwise the portal caller receives a Success
        // response with a URI to a file that was never written.
        ok = false;
    }
    if path.is_none() || also_clipboard {
        let mut bytes = Vec::new();
        match save_rgba_to_buffer(img, &mut bytes) {
            Ok(()) => cmds.push(clipboard::write_data(ScreenshotBytes { bytes })),
            Err(e) => {
                log::error!("Failed to encode screenshot for the clipboard: {e:?}");
                // Only a failure when the clipboard was the one destination.
                ok &= path.is_some();
            }
        }
    }
    ok
}

/// The selection as one image at the capture's own scale, annotations burned in.
fn render_selection(
    choice: Choice,
    mut images: std::collections::HashMap<String, super::ScreenshotImage>,
    outputs: &[OutputState],
    annotations: &[Box<dyn ToolOperation>],
) -> Option<RgbaImage> {
    match choice {
        Choice::Output(Some(output_name)) => {
            let Some(img) = images.remove(&output_name) else {
                log::error!("Failed to find output {output_name}");
                return None;
            };
            let mut img = img.rgba;
            if let Some(output) = outputs.iter().find(|o| o.name == output_name) {
                let scale = img.width() as f32 / output.logical_size.0 as f32;
                rasterize(&mut img, annotations, &output.rect(), scale);
            }
            Some(img)
        }
        Choice::Rectangle(r, _) if r.dimensions().is_some() => {
            // Physical pixels at the scale of the first output the region touches.
            let target_scale = outputs
                .iter()
                .find_map(|output| {
                    let img = images.get(&output.name)?;
                    r.intersect(output.rect())?;
                    Some(img.rgba.width() as f32 / output.logical_size.0 as f32)
                })
                .unwrap_or(1.0);
            let frames = outputs
                .iter()
                .filter_map(|output| {
                    let img = images.get(&output.name)?;
                    let (cropped, visible, _) = crop_to_output(&img.rgba, output, r)?;
                    Some((cropped, visible.scaled(target_scale)))
                })
                .collect();
            let mut img = combined_image(r.scaled(target_scale), frames);
            rasterize(&mut img, annotations, &r, target_scale);
            Some(img)
        }
        // An undragged rectangle: every output stitched into one image.
        Choice::Rectangle(..) => {
            let frames = outputs
                .iter()
                .filter_map(|output| Some((images.remove(&output.name)?.rgba, output.rect())))
                .collect();
            let img = stitch_outputs(frames);
            if img.is_none() {
                log::error!("No outputs available for all-screens capture");
            }
            img
        }
        Choice::Output(None) | Choice::AllScreens => None,
    }
}

pub fn save(app: &mut App) -> cosmic::Task<crate::app::Msg> {
    let destroy_cmds: Vec<cosmic::Task<crate::app::Msg>> = app
        .outputs
        .iter()
        .map(|o| destroy_layer_surface(o.id))
        .collect();
    let Some(capture) = app.capture.take() else {
        log::error!("No capture in progress");
        return cosmic::Task::batch(destroy_cmds);
    };
    let Capture {
        portal,
        output_images,
        selection,
        annotations,
        ui,
        ..
    } = capture;
    remember_capture(crate::config::CaptureMode::Screenshot, &selection.choice);

    let mut also_copy_to_clipboard = selection.also_copy_to_clipboard;
    let mut image_path = get_img_path(selection.location, &ui.custom_save_path);
    // A portal caller expects a file URI. Keep the clipboard choice and hand it a temporary file.
    if portal.expects_response && image_path.is_none() {
        image_path = Some(std::env::temp_dir().join(img_file_name()));
        also_copy_to_clipboard = true;
    }

    // All-screens is an undragged rectangle: every output stitched into one image.
    let choice = match selection.choice {
        Choice::AllScreens => Choice::Rectangle(Rect::default(), DragState::None),
        other => other,
    };

    let mut cmds = Vec::new();
    let image = render_selection(
        choice,
        output_images,
        &app.outputs,
        annotations.stack.operations(),
    );
    let success = image
        .is_some_and(|img| deliver(&img, image_path.as_ref(), also_copy_to_clipboard, &mut cmds));

    let response = match image_path.filter(|_| success) {
        // A proper file:// URI: correct slash count and percent-encoding.
        Some(path) => match url::Url::from_file_path(&path) {
            Ok(url) => PortalResponse::Success(ScreenshotResult {
                uri: url.to_string(),
            }),
            Err(()) => {
                log::error!("Could not build a file URI for '{}'", path.display());
                PortalResponse::Other
            }
        },
        None => PortalResponse::Other,
    };

    send_portal_response(portal.tx, portal.expects_response, response);
    // Clipboard writes go first: set_selection needs keyboard focus, which
    // destroying the overlay surfaces releases.
    cmds.extend(destroy_cmds);
    cosmic::Task::batch(cmds)
}

/// Close the session, telling a waiting portal caller it was cancelled. Returns
/// the tasks that take the overlay down.
pub fn dismiss(app: &mut App) -> Vec<cosmic::Task<crate::app::Msg>> {
    let cmds = app
        .outputs
        .iter()
        .map(|o| destroy_layer_surface(o.id))
        .collect();
    match app.capture.take() {
        Some(capture) => send_portal_response(
            capture.portal.tx,
            capture.portal.expects_response,
            PortalResponse::Cancelled,
        ),
        None => log::error!("No capture in progress"),
    }
    cmds
}

pub fn cancel(app: &mut App) -> cosmic::Task<crate::app::Msg> {
    if crate::recording::is_recording() {
        log::info!("Canceling - stopping active recording");
        if let Err(e) = crate::recording::stop_recording() {
            log::error!("Failed to stop recording: {e}");
        }
    }
    cosmic::Task::batch(dismiss(app))
}

/// Persist what this capture did, for the next one to start from.
pub fn remember_capture(mode: crate::config::CaptureMode, choice: &Choice) {
    let target = crate::config::LastTarget::from(choice);
    Config::store_many(move |tx| {
        use cosmic::cosmic_config::ConfigSet;
        tx.set("last_mode", mode)?;
        tx.set("last_target", target)
    });
}

/// Start where the last capture left off: the same mode, and the same target
/// when it still fits the current screens.
fn restore_last_capture(app: &App, capture: &mut Capture) {
    let config = Config::load();
    let video = config.last_mode == crate::config::CaptureMode::Video;
    capture.ui.is_video_mode = video;
    let screen = app
        .outputs
        .iter()
        .map(crate::app::OutputState::rect)
        .reduce(Rect::union);
    match config.last_target {
        crate::config::LastTarget::Region(region) => {
            if let Some(fit) = screen.and_then(|s| s.intersect(region))
                && fit.dimensions().is_some()
            {
                capture.selection.choice = Choice::Rectangle(fit, DragState::default());
            }
        }
        crate::config::LastTarget::Output(name) => {
            if let Some(index) = app.outputs.iter().position(|o| o.name == name) {
                capture.selection.choice = Choice::Output(Some(name));
                capture.selection.focused_output_index = index;
                capture.selection.has_mouse_entered = true;
            }
        }
        // Every screen cannot be recorded.
        crate::config::LastTarget::AllScreens if !video => {
            capture.selection.choice = Choice::AllScreens;
        }
        crate::config::LastTarget::AllScreens | crate::config::LastTarget::None => {}
    }
    // Open with the toolbar sections already settled instead of animating them in.
    capture.ui.toolbar_anim = crate::capture::ToolbarAnim::settled(
        crate::capture::ToolbarSections::of(crate::capture::ToolbarMode {
            annotating: false,
            video,
            has_selection: capture.selection.choice.has_selection(),
            annotation_selected: false,
            can_ocr: capture.ui.tesseract_available
                && matches!(&capture.selection.choice, Choice::Rectangle(r, _) if r.dimensions().is_some()),
        }),
    );
}

pub fn start(app: &mut App, capture: Capture) -> cosmic::Task<crate::app::Msg> {
    if app.outputs.len() != capture.output_images.len() {
        log::warn!(
            "Screenshot output count mismatch: {} outputs vs {} images, proceeding anyway (monitor reconnect?)",
            app.outputs.len(),
            capture.output_images.len()
        );
        log::warn!("Screenshot outputs: {:?}", app.outputs);
        log::warn!(
            "Screenshot images: {:?}",
            capture.output_images.keys().collect::<Vec<_>>()
        );
    }

    let mut capture = capture;
    restore_last_capture(app, &mut capture);
    // Create windows unless a session with live windows already exists.
    let old_args = app.capture.replace(capture);
    crate::capture::text::sync_format_models(app);
    let need_windows = match &old_args {
        None => true,
        Some(old) => {
            // If old session was recording, windows were destroyed so we need to recreate
            old.ui.is_recording
        }
    };

    // Clean up old portal response channel if we're replacing capture
    if let Some(old) = old_args {
        let tx = old.portal.tx;
        tokio::spawn(async move {
            // Send cancelled to the old portal request so D-Bus doesn't hang
            let _ = tx.send(PortalResponse::Cancelled).await;
        });
    }

    // Clean up recording indicator if it's still active
    // (Possible when the recording was stopped over D-Bus.)
    let indicator_cleanup = if let Some(mut indicator) = app.recording_indicator.take() {
        indicator.destroy_surfaces(&app.outputs)
    } else {
        cosmic::Task::none()
    };

    if need_windows {
        log::info!("Creating new screenshot windows");
        // Generate fresh window IDs for this session
        for output in &mut app.outputs {
            output.id = window::Id::unique();
        }

        // No outputs yet: OutputEvent::Created will create the surface.
        if app.outputs.is_empty() {
            log::info!(
                "Outputs not yet known, deferring window creation until OutputEvent::Created"
            );
            app.screenshot_windows_pending = true;
            return indicator_cleanup;
        }
        app.screenshot_windows_pending = false;

        let cmds = app.open_overlay();

        // Detect encoders asynchronously so UI appears immediately
        let encoder_task = cosmic::Task::perform(
            async {
                // Run encoder detection in a blocking task to not block the async runtime
                tokio::task::spawn_blocking(|| {
                    use crate::recording::encoder::detect_encoders;
                    detect_encoders().unwrap_or_default()
                })
                .await
                .unwrap_or_default()
            },
            |encoders| {
                crate::app::Msg::Screenshot(crate::capture::msg::Msg::Settings(
                    crate::capture::msg::SettingsMsg::EncodersDetected(encoders),
                ))
            },
        );

        indicator_cleanup.chain(cmds).chain(encoder_task)
    } else {
        log::info!("Existing screenshot capture updated (windows already exist)");
        indicator_cleanup
    }
}

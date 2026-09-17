// SPDX-License-Identifier: GPL-3.0-only

//! Starting a capture, and saving or discarding its result.

use super::Capture;
use crate::app::App;
use crate::config::{Config, SaveLocationChoice};
use crate::dbus::PortalResponse;
use crate::dbus::ScreenshotResult;
use crate::fl;
use crate::geometry::{Choice, DragState, ImageSaveLocation, Rect, RectDimension};
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

impl ScreenshotBytes {
    const fn new(bytes: Vec<u8>) -> Self {
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

pub fn save_rgba(img: &RgbaImage, path: &PathBuf) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(path)?;
    Ok(write_png(&mut file, img)?)
}

pub fn save_rgba_to_buffer(img: &RgbaImage, buffer: &mut Vec<u8>) -> anyhow::Result<()> {
    Ok(write_png(buffer, img)?)
}

pub fn get_img_path(location: ImageSaveLocation, custom_dir: Option<&str>) -> Option<PathBuf> {
    let mut path = match (location, custom_dir) {
        // Clipboard returns None (no file path). Check it first.
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
    path.push(img_file_name());

    Some(path)
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
    let physical = |r: Rect| Rect {
        left: (r.left as f32 * scale) as i32,
        top: (r.top as f32 * scale) as i32,
        right: (r.right as f32 * scale) as i32,
        bottom: (r.bottom as f32 * scale) as i32,
    };
    let bounds = frames.iter().map(|(_, r)| *r).reduce(Rect::union)?;
    let frames = frames
        .into_iter()
        .map(|(img, r)| (img, physical(r)))
        .collect();
    Some(combined_image(physical(bounds), frames))
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

pub fn handle_capture_inner(app: &mut App) -> cosmic::Task<crate::app::Msg> {
    let destroy_cmds: Vec<cosmic::Task<crate::app::Msg>> = app
        .outputs
        .iter()
        .map(|o| destroy_layer_surface(o.id))
        .collect();
    let mut cmds: Vec<cosmic::Task<crate::app::Msg>> = Vec::new();
    let Some(capture) = app.capture.take() else {
        log::error!("No capture in progress");
        return cosmic::Task::batch(destroy_cmds);
    };
    let outputs = app.outputs.clone();
    let Capture {
        portal,
        output_images: mut images,
        selection,
        annotations: args_annotations,
        ui,
        ..
    } = capture;
    let expects_response = portal.expects_response;
    let tx = portal.tx;
    let choice = selection.choice;
    remember_capture(crate::config::CaptureMode::Screenshot, &choice);
    let location = selection.location;
    let operations = args_annotations.stack;
    let mut also_copy_to_clipboard = selection.also_copy_to_clipboard;

    let annotations = operations.operations();

    // Determine custom save path based on save location setting
    let custom_dir = match ui.save_location_setting {
        SaveLocationChoice::Custom if !ui.custom_save_path.is_empty() => {
            Some(ui.custom_save_path.as_str())
        }
        _ => None,
    };

    let mut success = true;
    let mut image_path = get_img_path(location, custom_dir);
    // A portal caller expects a file URI. Keep the clipboard choice and hand it a temporary file.
    if expects_response && image_path.is_none() {
        image_path = Some(std::env::temp_dir().join(img_file_name()));
        also_copy_to_clipboard = true;
    }

    // All-screens is an undragged rectangle: every output stitched into one image.
    let choice = match choice {
        Choice::AllScreens => Choice::Rectangle(Rect::default(), DragState::None),
        other => other,
    };

    match choice {
        Choice::Output(Some(output_name)) => {
            if let Some(img) = images.remove(&output_name) {
                let mut final_img = img.rgba;

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
                        rasterize(&mut final_img, annotations, &output_rect, scale);
                    }
                }

                if let Some(ref image_path) = image_path {
                    if let Err(err) = save_rgba(&final_img, image_path) {
                        log::error!("Failed to capture screenshot: {err:?}");
                        // Must flag failure, otherwise the portal caller receives a
                        // Success response with a URI to a file that was never written.
                        success = false;
                    }
                    // Also copy to clipboard if enabled
                    if also_copy_to_clipboard {
                        let mut buffer = Vec::new();
                        if let Err(e) = save_rgba_to_buffer(&final_img, &mut buffer) {
                            log::error!("Failed to save screenshot to buffer: {e:?}");
                        } else {
                            cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                        }
                    }
                } else {
                    let mut buffer = Vec::new();
                    if let Err(e) = save_rgba_to_buffer(&final_img, &mut buffer) {
                        log::error!("Failed to save screenshot to buffer: {e:?}");
                        success = false;
                    } else {
                        cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                    }
                }
            } else {
                log::error!("Failed to find output {output_name}");
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
                    rasterize(&mut img, annotations, &r, target_scale);
                }

                if let Some(ref image_path) = image_path {
                    if let Err(_err) = save_rgba(&img, image_path) {
                        success = false;
                    }
                    // Also copy to clipboard if enabled
                    if also_copy_to_clipboard {
                        let mut buffer = Vec::new();
                        if let Err(e) = save_rgba_to_buffer(&img, &mut buffer) {
                            log::error!("Failed to save screenshot to buffer: {e:?}");
                        } else {
                            cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                        }
                    }
                } else {
                    let mut buffer = Vec::new();
                    if let Err(e) = save_rgba_to_buffer(&img, &mut buffer) {
                        log::error!("Failed to save screenshot to buffer: {e:?}");
                        success = false;
                    } else {
                        cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                    }
                }
            } else {
                // Empty selection: capture all screens combined
                let frames = images
                    .into_iter()
                    .filter_map(|(name, raw_img)| {
                        let output = outputs.iter().find(|o| o.name == name)?;
                        Some((raw_img.rgba, output.rect()))
                    })
                    .collect();
                if let Some(img) = stitch_outputs(frames) {
                    if let Some(ref image_path) = image_path {
                        if let Err(err) = save_rgba(&img, image_path) {
                            log::error!("Failed to capture screenshot: {err:?}");
                            success = false;
                        }
                        // Also copy to clipboard if enabled
                        if also_copy_to_clipboard {
                            let mut buffer = Vec::new();
                            if let Err(e) = save_rgba_to_buffer(&img, &mut buffer) {
                                log::error!("Failed to save screenshot to buffer: {e:?}");
                            } else {
                                cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                            }
                        }
                    } else {
                        let mut buffer = Vec::new();
                        if let Err(e) = save_rgba_to_buffer(&img, &mut buffer) {
                            log::error!("Failed to save screenshot to buffer: {e:?}");
                            success = false;
                        } else {
                            cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)));
                        }
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
        // A proper file:// URI: correct slash count and percent-encoding.
        if let Ok(url) = url::Url::from_file_path(&image_path1) {
            PortalResponse::Success(ScreenshotResult {
                uri: url.to_string(),
            })
        } else {
            log::error!("Could not build a file URI for '{}'", image_path1.display());
            PortalResponse::Other
        }
    } else {
        PortalResponse::Other
    };

    send_portal_response(tx, expects_response, response);
    // Clipboard writes go first: set_selection needs keyboard focus, which
    // destroying the overlay surfaces releases.
    cmds.extend(destroy_cmds);
    cosmic::Task::batch(cmds)
}

pub fn handle_cancel_inner(app: &mut App) -> cosmic::Task<crate::app::Msg> {
    // Stop recording if active
    if crate::recording::is_recording() {
        log::info!("Canceling - stopping active recording");
        if let Err(e) = crate::recording::stop_recording() {
            log::error!("Failed to stop recording: {e}");
        }
    }

    let cmds = app.outputs.iter().map(|o| destroy_layer_surface(o.id));
    let Some(capture) = app.capture.take() else {
        log::error!("No capture in progress");
        return cosmic::Task::batch(cmds);
    };
    let Capture { portal, .. } = capture;
    send_portal_response(
        portal.tx,
        portal.expects_response,
        PortalResponse::Cancelled,
    );

    cosmic::Task::batch(cmds)
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

    app.location_options = vec![
        fl!("save-to", "clipboard"),
        fl!("save-to", "pictures"),
        fl!("save-to", "documents"),
    ];

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

/// Recreate the layer surfaces after they were destroyed, for example for a file dialog.
pub fn recreate_screenshot_surfaces(app: &mut App) -> cosmic::Task<crate::app::Msg> {
    app.open_overlay()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stitch_outputs_places_frames_by_logical_rect() {
        // Two 1x scale outputs side by side, red then blue.
        let red = RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]));
        let img = stitch_outputs(vec![
            (red, Rect::new(0, 0, 2, 2)),
            (blue, Rect::new(2, 0, 4, 2)),
        ])
        .unwrap();
        assert_eq!(img.dimensions(), (4, 2));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(3, 1).0, [0, 0, 255, 255]);
        assert!(stitch_outputs(vec![]).is_none());
    }
}

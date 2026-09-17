// SPDX-License-Identifier: GPL-3.0-only

//! OCR and QR detection on the selection.

use super::flow::rasterize;
use super::flow::send_portal_response;
use crate::app::App;
use crate::capture::msg::Msg;
use crate::capture::msg::OcrMsg;
use crate::capture::msg::QrMsg;
use crate::dbus::PortalResponse;
use crate::fl;
use crate::geometry::{Choice, Rect};
use cosmic::iced::platform_specific::shell::commands::layer_surface::destroy_layer_surface;
use cosmic::iced::runtime::clipboard;
use image::RgbaImage;
use std::collections::HashMap;

// OCR via tesseract.

/// OCR text overlay metadata
#[derive(Clone, Debug, PartialEq)]
pub struct OcrTextOverlay {
    /// Bounding box in logical coordinates (relative to output)
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    /// Recognized text for this region
    pub text: String,
    /// Block number for coloring
    pub block_num: i32,
    /// Which output this overlay belongs to
    pub output_name: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum OcrStatus {
    #[default]
    Idle,
    DownloadingModels,
    Running,
    Done(String, Vec<OcrTextOverlay>),
    Error(String),
}

#[derive(Clone, Debug)]
pub struct OcrMapping {
    /// Top-left of the cropped OCR region in logical coordinates
    pub origin: (f32, f32),
    /// Size of the cropped OCR region in logical coordinates
    pub size: (f32, f32),
    /// Pixels-per-logical-unit for this output image
    pub scale: f32,
    /// Output name this mapping belongs to
    pub output_name: String,
}

/// Check if OCR models need to be downloaded.
/// For rusty-tesseract, this always returns false as it uses system tesseract.
pub const fn models_need_download() -> bool {
    // rusty-tesseract uses system tesseract, no model download needed
    false
}

/// Check if tesseract is installed and available on the system.
pub fn is_tesseract_available() -> bool {
    std::process::Command::new("tesseract")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Run OCR on an image and return the status with detected text and overlays.
pub fn run_ocr_on_image_with_status(img: &RgbaImage, mapping: OcrMapping) -> OcrStatus {
    use rusty_tesseract::{Args as TessArgs, Image};

    if mapping.scale <= 0.0 {
        return OcrStatus::Error(fl!("invalid-ocr-scale"));
    }

    log::info!(
        "Running OCR with rusty-tesseract on {}x{} image...",
        img.width(),
        img.height()
    );

    // Convert RgbaImage to DynamicImage
    let dynamic_img = image::DynamicImage::ImageRgba8(img.clone());

    // For small images, upscale to improve OCR accuracy on small text
    // Tesseract works best with text that's at least 10-12 pixels tall
    let min_dimension = img.width().min(img.height());
    let (processed_img, upscale_factor) = if min_dimension < 100 {
        // Very small selection: upscale 4x
        let new_width = img.width() * 4;
        let new_height = img.height() * 4;
        log::info!("Upscaling small image 4x to {new_width}x{new_height}");
        (
            dynamic_img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3),
            4.0_f32,
        )
    } else if min_dimension < 200 {
        // Small selection: upscale 2x
        let new_width = img.width() * 2;
        let new_height = img.height() * 2;
        log::info!("Upscaling small image 2x to {new_width}x{new_height}");
        (
            dynamic_img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3),
            2.0_f32,
        )
    } else {
        (dynamic_img, 1.0_f32)
    };

    // Create rusty-tesseract Image from DynamicImage
    let tess_img = match Image::from_dynamic_image(&processed_img) {
        Ok(img) => img,
        Err(e) => {
            return OcrStatus::Error(fl!("tesseract-image-error", error = e.to_string()));
        }
    };

    // Configure tesseract arguments
    // Use higher DPI for better small text recognition
    let dpi = if min_dimension < 200 { 300 } else { 150 };
    let tess_args = TessArgs {
        lang: "eng".to_string(),
        config_variables: HashMap::new(),
        dpi: Some(dpi),
        psm: Some(11), // Fully automatic page segmentation
        oem: Some(3),  // Default OCR Engine Mode
    };

    // Run OCR for text
    let text_result = rusty_tesseract::image_to_string(&tess_img, &tess_args);
    let data_result = rusty_tesseract::image_to_data(&tess_img, &tess_args);

    let mut overlays = Vec::new();
    if let Ok(data_output) = data_result {
        log::info!("Tesseract returned {} data entries", data_output.data.len());

        // Group words by block_num to create block-level overlays
        let mut blocks: std::collections::HashMap<i32, Vec<_>> = std::collections::HashMap::new();
        for d in data_output
            .data
            .into_iter()
            .filter(|d| !d.text.trim().is_empty() && d.conf > 0.0)
        {
            blocks.entry(d.block_num).or_default().push(d);
        }

        for (block_num, words) in blocks {
            if words.is_empty() {
                continue;
            }

            // Calculate bounding box for the entire block
            let mut min_left = i32::MAX;
            let mut min_top = i32::MAX;
            let mut max_right = i32::MIN;
            let mut max_bottom = i32::MIN;

            // Sort words by line_num then word_num for proper text ordering
            let mut sorted_words = words;
            sorted_words.sort_by(|a, b| {
                a.line_num
                    .cmp(&b.line_num)
                    .then(a.word_num.cmp(&b.word_num))
            });

            // Build combined text and bounding box
            let mut text_parts: Vec<String> = Vec::new();
            let mut current_line = -1;

            for word in &sorted_words {
                min_left = min_left.min(word.left);
                min_top = min_top.min(word.top);
                max_right = max_right.max(word.left + word.width);
                max_bottom = max_bottom.max(word.top + word.height);

                if word.line_num == current_line {
                    text_parts.push(" ".to_string());
                } else {
                    if current_line != -1 {
                        text_parts.push(" ".to_string());
                    }
                    current_line = word.line_num;
                }
                text_parts.push(word.text.clone());
            }

            let block_text = text_parts.concat().trim().to_string();
            if block_text.is_empty() {
                continue;
            }

            // Convert bounding box to output-relative logical coords
            // Divide by upscale_factor first since tesseract coords are in upscaled image space
            let left = mapping.origin.0 + min_left as f32 / upscale_factor / mapping.scale;
            let top = mapping.origin.1 + min_top as f32 / upscale_factor / mapping.scale;
            let width = (max_right - min_left) as f32 / upscale_factor / mapping.scale;
            let height = (max_bottom - min_top) as f32 / upscale_factor / mapping.scale;

            log::info!(
                "OCR block {block_num}: '{block_text}' at ({left}, {top}, {width}x{height})"
            );
            overlays.push(OcrTextOverlay {
                left,
                top,
                width,
                height,
                text: block_text,
                block_num,
                output_name: mapping.output_name.clone(),
            });
        }
        log::info!("Generated {} block-level OCR overlays", overlays.len());
    }

    match text_result {
        Ok(text) => {
            let text = text.trim().to_string();
            let no_text = fl!("no-text-detected");
            let text = if text.is_empty() {
                no_text.clone()
            } else {
                text
            };
            // If no blocks found, create a fallback overlay covering the whole selection
            if overlays.is_empty() && !text.is_empty() && text != no_text {
                overlays.push(OcrTextOverlay {
                    left: mapping.origin.0,
                    top: mapping.origin.1,
                    width: mapping.size.0,
                    height: mapping.size.1,
                    text: text.clone(),
                    block_num: 0,
                    output_name: mapping.output_name,
                });
            }
            OcrStatus::Done(text, overlays)
        }
        Err(e) => OcrStatus::Error(fl!("tesseract-ocr-error", error = e.to_string())),
    }
}

// QR code detection via rqrr.

/// Detected QR code with position and content
#[derive(Clone, Debug)]
pub struct DetectedQrCode {
    /// Center position in logical coordinates (relative to output)
    pub center_x: f32,
    pub center_y: f32,
    /// The decoded content of the QR code
    pub content: String,
    /// Which output this QR code is on
    pub output_name: String,
    /// Half the code's size in logical units, for outlining it and placing the badge.
    pub half_width: f32,
    pub half_height: f32,
}

/// Detect QR codes in an image at a specific resolution
/// `max_dim`: maximum dimension to downsample to (0 = no downsampling)
pub fn detect_qr_codes_at_resolution(
    img: &RgbaImage,
    output_name: &str,
    scale: f32,
    max_dim: u32,
) -> Vec<DetectedQrCode> {
    use rqrr::PreparedImage;

    let (orig_w, orig_h) = (img.width(), img.height());
    let downsample_factor = if max_dim > 0 && (orig_w > max_dim || orig_h > max_dim) {
        orig_w.max(orig_h) as f32 / max_dim as f32
    } else {
        1.0
    };

    let gray = if downsample_factor > 1.0 {
        let new_w = (orig_w as f32 / downsample_factor) as u32;
        let new_h = (orig_h as f32 / downsample_factor) as u32;
        let resized =
            image::imageops::resize(img, new_w, new_h, image::imageops::FilterType::Nearest);
        image::DynamicImage::ImageRgba8(resized).to_luma8()
    } else {
        image::DynamicImage::ImageRgba8(img.clone()).to_luma8()
    };

    let mut prepared = PreparedImage::prepare(gray);
    let grids = prepared.detect_grids();

    let mut results = Vec::new();
    for grid in grids {
        if let Ok((_, content)) = grid.decode() {
            let bounds = &grid.bounds;
            let cx = (bounds[0].x + bounds[1].x + bounds[2].x + bounds[3].x) as f32 / 4.0;
            let cy = (bounds[0].y + bounds[1].y + bounds[2].y + bounds[3].y) as f32 / 4.0;

            let left = bounds.iter().map(|p| p.x).min().unwrap_or(0) as f32;
            let right = bounds.iter().map(|p| p.x).max().unwrap_or(0) as f32;
            let top = bounds.iter().map(|p| p.y).min().unwrap_or(0) as f32;
            let bottom = bounds.iter().map(|p| p.y).max().unwrap_or(0) as f32;

            results.push(DetectedQrCode {
                center_x: (cx * downsample_factor) / scale,
                center_y: (cy * downsample_factor) / scale,
                content,
                output_name: output_name.to_string(),
                half_width: ((right - left) * downsample_factor) / scale / 2.0,
                half_height: ((bottom - top) * downsample_factor) / scale / 2.0,
            });
        }
    }

    results
}

/// Check if a QR code is a duplicate (same content at similar position)
pub fn is_duplicate_qr(existing: &[DetectedQrCode], new: &DetectedQrCode) -> bool {
    const POSITION_THRESHOLD: f32 = 50.0; // pixels
    existing.iter().any(|e| {
        e.content == new.content
            && e.output_name == new.output_name
            && (e.center_x - new.center_x).abs() < POSITION_THRESHOLD
            && (e.center_y - new.center_y).abs() < POSITION_THRESHOLD
    })
}

// Classifies scanned QR payloads and the actions available for each.

/// A scanned payload, recognized as far as we can.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrKind {
    /// A web address
    Url,
    /// Anything we have not taught ourselves to read
    Text,
}

impl QrKind {
    /// Classify a scanned payload.
    #[must_use]
    pub fn of(content: &str) -> Self {
        let c = content.trim();
        if c.starts_with("http://") || c.starts_with("https://") || c.starts_with("www.") {
            Self::Url
        } else {
            Self::Text
        }
    }
}

/// Something the user can do with a scanned code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrAction {
    /// Put the payload on the clipboard
    Copy,
    /// Hand the payload to the desktop to open
    Open,
    /// Put the code away
    Dismiss,
}

impl QrAction {
    /// The symbolic icon this action is drawn with.
    #[must_use]
    pub const fn icon_name(&self) -> &'static str {
        match self {
            Self::Copy => "edit-copy-symbolic",
            Self::Open => "go-next-symbolic",
            Self::Dismiss => "window-close-symbolic",
        }
    }
}

/// The actions offered for a payload, in display order. Dismiss is last.
#[must_use]
pub fn actions_for(content: &str) -> Vec<QrAction> {
    let mut actions = Vec::new();
    if QrKind::of(content) == QrKind::Url {
        actions.push(QrAction::Open);
    }
    actions.push(QrAction::Copy);
    actions.push(QrAction::Dismiss);
    actions
}

/// Handle QR detection messages
pub fn handle_qr_msg(app: &mut App, msg: QrMsg) -> cosmic::Task<crate::app::Msg> {
    match msg {
        QrMsg::Dismiss => {
            if let Some(handle) = app.qr_scan.take() {
                handle.abort();
            }
            if let Some(capture) = app.capture.as_mut() {
                capture.detection.qr_codes.clear();
                capture.detection.qr_scanning = false;
            }
            cosmic::Task::none()
        }
        QrMsg::Requested => handle_qr_requested_inner(app, true),
        QrMsg::Detected(codes) => handle_qr_detected_inner(app, codes),
        QrMsg::CopyAndClose => handle_qr_copy_and_close_inner(app),
    }
}

/// Handle OCR detection messages
pub fn handle_ocr_msg(app: &mut App, msg: OcrMsg) -> cosmic::Task<crate::app::Msg> {
    match msg {
        OcrMsg::Requested => handle_ocr_requested_inner(app),
        OcrMsg::Status(generation, status) => handle_ocr_status_inner(app, generation, status),
        OcrMsg::StatusClear => handle_ocr_status_clear_inner(app),
        OcrMsg::CopyAndClose => handle_ocr_copy_and_close_inner(app),
    }
}

/// Scan the selection for QR codes. `manual` scans may put the tools away.
/// Automatic ones must not.
pub fn handle_qr_requested_inner(app: &mut App, manual: bool) -> cosmic::Task<crate::app::Msg> {
    // A scan already in flight is for a selection that no longer exists.
    if let Some(handle) = app.qr_scan.take() {
        handle.abort();
    }

    // Clear previous state and start QR scanning (keep redactions)
    if let Some(capture) = app.capture.as_mut() {
        capture.detection.qr_codes.clear();
        capture.detection.qr_scanning = true;
        capture.detection.ocr_overlays.clear();
        capture.detection.ocr_status = OcrStatus::Idle;
        capture.detection.ocr_text = None;
        if manual {
            capture.clear_shapes();
            capture.disable_all_modes();
            capture.close_all_popups();
        }
    }

    // Get the selection and run QR detection on that area
    if let Some(capture) = app.capture.as_ref() {
        let annotations = capture.annotations.stack.clone();
        let outputs_clone = app.outputs.clone();

        // One entry per region: a selection can span several outputs.
        let qr_params: Vec<(RgbaImage, String, f32, f32, f32, Rect)> = match &capture
            .selection
            .choice
        {
            Choice::Rectangle(rect, _) if rect.width() > 0 && rect.height() > 0 => {
                let mut params = Vec::new();
                for output in &app.outputs {
                    if let Some(img) = capture.output_images.get(&output.name) {
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
                            params.push((
                                cropped,
                                output.name.clone(),
                                scale,
                                origin_x,
                                origin_y,
                                intersection,
                            ));
                        }
                    }
                }
                params
            }
            Choice::Output(Some(output_name)) => capture
                .output_images
                .get(output_name)
                .and_then(|img| {
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
                .into_iter()
                .collect(),

            // Every screen, each scanned in its own coordinates.
            Choice::AllScreens => outputs_clone
                .iter()
                .filter_map(|output| {
                    capture.output_images.get(&output.name).map(|img| {
                        let scale = img.rgba.width() as f32 / output.logical_size.0 as f32;
                        let output_rect = Rect {
                            left: output.logical_pos.0,
                            top: output.logical_pos.1,
                            right: output.logical_pos.0 + output.logical_size.0 as i32,
                            bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                        };
                        (
                            img.rgba.clone(),
                            output.name.clone(),
                            scale,
                            0.0,
                            0.0,
                            output_rect,
                        )
                    })
                })
                .collect(),

            _ => Vec::new(),
        };

        if !qr_params.is_empty() {
            let mut qr_detection_tasks = Vec::new();

            for (mut cropped, output_name, scale, origin_x, origin_y, selection_rect) in qr_params {
                // Apply annotations to the image before QR scanning
                if !annotations.is_empty() {
                    rasterize(
                        &mut cropped,
                        annotations.operations(),
                        &selection_rect,
                        scale,
                    );
                }
                // Spawn progressive QR detection tasks (3 passes with increasing resolution)
                let resolutions = [500u32, 1500, 0]; // 0 = full resolution

                for max_dim in resolutions {
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
                        move |qr_codes| crate::app::Msg::Screenshot(Msg::qr_detected(qr_codes)),
                    );
                    qr_detection_tasks.push(task);
                }
            }

            // Keep the handle so the next selection can abort this scan.
            let (task, handle) = cosmic::Task::batch(qr_detection_tasks).abortable();
            app.qr_scan = Some(handle);
            return task;
        }
    }
    cosmic::Task::none()
}

pub fn handle_qr_detected_inner(
    app: &mut App,
    new_qr_codes: Vec<DetectedQrCode>,
) -> cosmic::Task<crate::app::Msg> {
    if let Some(capture) = app.capture.as_mut() {
        // Scanning pass completed: hide scanning indicator after first pass
        capture.detection.qr_scanning = false;

        // Merge new QR codes, avoiding duplicates
        for qr in new_qr_codes {
            if !is_duplicate_qr(&capture.detection.qr_codes, &qr) {
                capture.detection.qr_codes.push(qr);
            }
        }
    }
    cosmic::Task::none()
}

pub fn handle_ocr_requested_inner(app: &mut App) -> cosmic::Task<crate::app::Msg> {
    // Check if models need downloading and set appropriate status
    let needs_download = models_need_download();
    if let Some(capture) = app.capture.as_mut() {
        capture.detection.ocr_status = if needs_download {
            OcrStatus::DownloadingModels
        } else {
            OcrStatus::Running
        };
        // Clear previous state (keep redactions)
        capture.detection.ocr_overlays.clear();
        capture.detection.ocr_text = None;
        capture.detection.qr_codes.clear();
        capture.clear_shapes();
        capture.disable_all_modes();
        capture.close_all_popups();
    }

    // Get the selection and run OCR on that area
    if let Some(capture) = app.capture.as_ref() {
        // The live ones only: anything undone is still in the history but is
        // not part of what is being read.
        let annotations = capture.annotations.stack.clone();
        let outputs_clone = app.outputs.clone();

        // Returns: (image, mapping, selection_rect_for_redactions, scale_for_redactions)
        let region_data: Option<(RgbaImage, OcrMapping, Rect, f32)> = match &capture
            .selection
            .choice
        {
            Choice::Rectangle(rect, _) if rect.width() > 0 && rect.height() > 0 => {
                // Collect image data for the selected rectangle
                let mut data = None;
                for output in &app.outputs {
                    if let Some(img) = capture.output_images.get(&output.name) {
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
                capture.output_images.get(output_name).and_then(|img| {
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
                rasterize(
                    &mut cropped_img,
                    annotations.operations(),
                    &selection_rect,
                    scale,
                );
            }

            // Run OCR in background with status updates. The result carries the
            // generation it belongs to, so clearing meanwhile discards it.
            let generation = app
                .capture
                .as_ref()
                .map_or(0, |c| c.detection.ocr_generation);
            return cosmic::Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        run_ocr_on_image_with_status(&cropped_img, mapping)
                    })
                    .await
                    .unwrap_or_else(|_| OcrStatus::Error("OCR task panicked".to_string()))
                },
                move |status| crate::app::Msg::Screenshot(Msg::ocr_status(generation, status)),
            );
        }
    }
    cosmic::Task::none()
}

pub fn handle_ocr_status_inner(
    app: &mut App,
    generation: u64,
    status: OcrStatus,
) -> cosmic::Task<crate::app::Msg> {
    // Results were cleared while this run was working: it is no longer wanted.
    if app
        .capture
        .as_ref()
        .is_some_and(|c| c.detection.ocr_generation != generation)
    {
        log::info!("Discarding OCR result from a cleared run");
        return cosmic::Task::none();
    }
    match &status {
        OcrStatus::Done(text, overlays) => {
            log::info!("OCR Result: {} ({} overlays)", text, overlays.len());
            if let Some(capture) = app.capture.as_mut() {
                capture.detection.ocr_status = status.clone();
                capture.detection.ocr_overlays = overlays.clone();
                // Store text for later copying when user clicks the button
                if !text.is_empty() && *text != fl!("no-text-detected") {
                    capture.detection.ocr_text = Some(text.clone());
                }
                log::info!(
                    "Stored {} overlays in capture",
                    capture.detection.ocr_overlays.len()
                );
            }
            // Don't auto-copy: user will click "copy text" button
        }
        OcrStatus::Error(err) => {
            log::error!("OCR Error: {err}");
            if let Some(capture) = app.capture.as_mut() {
                capture.detection.ocr_status = status;
                capture.detection.ocr_overlays.clear();
                capture.detection.ocr_text = None;
            }
        }
        _ => {
            if let Some(capture) = app.capture.as_mut() {
                capture.detection.ocr_status = status;
            }
        }
    }
    cosmic::Task::none()
}

pub fn handle_ocr_status_clear_inner(app: &mut App) -> cosmic::Task<crate::app::Msg> {
    if let Some(capture) = app.capture.as_mut() {
        capture.detection.ocr_status = OcrStatus::Idle;
    }
    cosmic::Task::none()
}

pub fn handle_ocr_copy_and_close_inner(app: &mut App) -> cosmic::Task<crate::app::Msg> {
    // Copy OCR text and close the app
    let mut cmds: Vec<cosmic::Task<crate::app::Msg>> = app
        .outputs
        .iter()
        .map(|o| destroy_layer_surface(o.id))
        .collect();

    if let Some(capture) = app.capture.take() {
        let tx = capture.portal.tx;
        let expects_response = capture.portal.expects_response;
        let ocr_text = capture.detection.ocr_text;

        if let Some(text) = ocr_text {
            cmds.push(clipboard::write(text));
        }

        send_portal_response(tx, expects_response, PortalResponse::Cancelled);
    }
    cosmic::Task::batch(cmds)
}

pub fn handle_qr_copy_and_close_inner(app: &mut App) -> cosmic::Task<crate::app::Msg> {
    // Copy first QR code content and close the app
    let mut cmds: Vec<cosmic::Task<crate::app::Msg>> = app
        .outputs
        .iter()
        .map(|o| destroy_layer_surface(o.id))
        .collect();

    if let Some(capture) = app.capture.take() {
        let tx = capture.portal.tx;
        let expects_response = capture.portal.expects_response;
        let qr_codes = capture.detection.qr_codes;

        // Copy first QR code content
        if let Some(qr) = qr_codes.first() {
            cmds.push(clipboard::write(qr.content.clone()));
        }

        send_portal_response(tx, expects_response, PortalResponse::Cancelled);
    }
    cosmic::Task::batch(cmds)
}

pub fn handle_open_url_inner(app: &mut App, url: String) -> cosmic::Task<crate::app::Msg> {
    // Open URL using xdg-open and close the screenshot tool
    log::info!("Opening URL: {url}");
    if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
        log::error!("Failed to open URL: {e}");
    }

    // Close the screenshot tool
    let cmds = app.outputs.iter().map(|o| destroy_layer_surface(o.id));
    let Some(capture) = app.capture.take() else {
        log::error!("No capture in progress");
        return cosmic::Task::batch(cmds);
    };
    send_portal_response(
        capture.portal.tx,
        capture.portal.expects_response,
        PortalResponse::Cancelled,
    );

    cosmic::Task::batch(cmds)
}

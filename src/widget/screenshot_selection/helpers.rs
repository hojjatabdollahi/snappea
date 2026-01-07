//! Helper functions for screenshot selection widget
//!
//! These functions extract common logic from the widget constructor
//! to improve readability and testability.

use std::collections::HashMap;

use crate::{
    capture::{image::ScreenshotImage, ocr::OcrTextOverlay, qr::DetectedQrCode},
    domain::{Choice, Rect},
};

/// Filter QR codes for a specific output
pub fn filter_qr_codes_for_output(
    qr_codes: &[DetectedQrCode],
    output_name: &str,
) -> Vec<(f32, f32, String)> {
    qr_codes
        .iter()
        .filter(|qr| qr.output_name == output_name)
        .map(|qr| (qr.center_x, qr.center_y, qr.content.clone()))
        .collect()
}

/// Filter OCR overlays for a specific output
pub fn filter_ocr_overlays_for_output(
    ocr_overlays: &[OcrTextOverlay],
    output_name: &str,
) -> Vec<(f32, f32, f32, f32, i32)> {
    ocr_overlays
        .iter()
        .filter(|o| o.output_name == output_name)
        .map(|o| (o.left, o.top, o.width, o.height, o.block_num))
        .collect()
}

/// Calculate selection rectangle relative to an output
///
/// Returns (x, y, width, height) in output-local coordinates, or None if no selection.
pub fn calculate_selection_rect(
    choice: &Choice,
    output_rect: Rect,
    output_logical_size: (u32, u32),
    toplevel_images: &HashMap<String, Vec<ScreenshotImage>>,
) -> Option<(f32, f32, f32, f32)> {
    match choice {
        Choice::Rectangle(r, _) => {
            if let Some(intersection) = r.intersect(output_rect) {
                let x = (intersection.left - output_rect.left) as f32;
                let y = (intersection.top - output_rect.top) as f32;
                let w = intersection.width() as f32;
                let h = intersection.height() as f32;
                if w > 0.0 && h > 0.0 {
                    Some((x, y, w, h))
                } else {
                    None
                }
            } else {
                None
            }
        }
        Choice::Window(win_output, Some(win_idx)) => {
            // For selected window mode, calculate where the window image will be drawn (centered)
            calculate_window_display_bounds(
                win_output,
                *win_idx,
                output_logical_size,
                toplevel_images,
            )
        }
        Choice::Output(Some(_)) => {
            // For confirmed output mode, the entire output is the selection area
            Some((
                0.0,
                0.0,
                output_logical_size.0 as f32,
                output_logical_size.1 as f32,
            ))
        }
        _ => None,
    }
}

/// Calculate window display bounds and scale for window mode
///
/// Returns (x, y, width, height) of where the window will be displayed.
fn calculate_window_display_bounds(
    win_output: &str,
    win_idx: usize,
    output_logical_size: (u32, u32),
    toplevel_images: &HashMap<String, Vec<ScreenshotImage>>,
) -> Option<(f32, f32, f32, f32)> {
    let img = toplevel_images.get(win_output)?.get(win_idx)?;

    let orig_width = img.rgba.width() as f32;
    let orig_height = img.rgba.height() as f32;
    let output_width = output_logical_size.0 as f32;
    let output_height = output_logical_size.1 as f32;

    // Step 1: Calculate the pre-scaled thumbnail size (same as SelectedImageWidget)
    let max_width = output_width * 0.85;
    let max_height = output_height * 0.85;
    let (thumb_width, thumb_height) = if orig_width > max_width || orig_height > max_height {
        let pre_scale = (max_width / orig_width).min(max_height / orig_height);
        (orig_width * pre_scale, orig_height * pre_scale)
    } else {
        (orig_width, orig_height)
    };

    // Step 2: Calculate display position (centering the thumbnail with 20px margin)
    let available_width = output_width - 20.0;
    let available_height = output_height - 20.0;
    let scale_x = available_width / thumb_width;
    let scale_y = available_height / thumb_height;
    let scale = scale_x.min(scale_y).min(1.0);

    let display_width = thumb_width * scale;
    let display_height = thumb_height * scale;
    let x = (output_width - display_width) / 2.0;
    let y = (output_height - display_height) / 2.0;

    Some((x, y, display_width, display_height))
}

/// Get window image reference and display info for pixelation preview
///
/// Returns (window_image, (x, y, width, height, display_to_original_scale))
#[allow(clippy::type_complexity)]
pub fn get_window_image_info<'a>(
    choice: &Choice,
    output_logical_size: (u32, u32),
    toplevel_images: &'a HashMap<String, Vec<ScreenshotImage>>,
) -> (
    Option<&'a ::image::RgbaImage>,
    Option<(f32, f32, f32, f32, f32)>,
) {
    let Choice::Window(win_output, Some(win_idx)) = choice else {
        return (None, None);
    };

    let Some(img) = toplevel_images
        .get(win_output)
        .and_then(|imgs| imgs.get(*win_idx))
    else {
        return (None, None);
    };

    let orig_width = img.rgba.width() as f32;
    let orig_height = img.rgba.height() as f32;
    let output_width = output_logical_size.0 as f32;
    let output_height = output_logical_size.1 as f32;

    // Step 1: Calculate the pre-scaled thumbnail size
    let max_width = output_width * 0.85;
    let max_height = output_height * 0.85;
    let (thumb_width, thumb_height) = if orig_width > max_width || orig_height > max_height {
        let pre_scale = (max_width / orig_width).min(max_height / orig_height);
        (orig_width * pre_scale, orig_height * pre_scale)
    } else {
        (orig_width, orig_height)
    };

    // Step 2: Calculate display position and scale
    let available_width = output_width - 20.0;
    let available_height = output_height - 20.0;
    let scale_x = available_width / thumb_width;
    let scale_y = available_height / thumb_height;
    let display_scale = scale_x.min(scale_y).min(1.0);

    let display_width = thumb_width * display_scale;
    let display_height = thumb_height * display_scale;
    let x = (output_width - display_width) / 2.0;
    let y = (output_height - display_height) / 2.0;

    // Total scale from display coords to ORIGINAL image pixels
    let display_to_original_scale = orig_width / display_width;

    (
        Some(&img.rgba),
        Some((
            x,
            y,
            display_width,
            display_height,
            display_to_original_scale,
        )),
    )
}

/// Create an output rect from logical position and size
pub fn create_output_rect(logical_pos: (i32, i32), logical_size: (u32, u32)) -> Rect {
    Rect {
        left: logical_pos.0,
        top: logical_pos.1,
        right: logical_pos.0 + logical_size.0 as i32,
        bottom: logical_pos.1 + logical_size.1 as i32,
    }
}

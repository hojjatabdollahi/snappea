//! Helper functions for screenshot selection widget
//!
//! These functions extract common logic from the widget constructor
//! to improve readability and testability.

use crate::{
    capture::{ocr::OcrTextOverlay, qr::DetectedQrCode},
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

/// Create an output rect from logical position and size
pub fn create_output_rect(logical_pos: (i32, i32), logical_size: (u32, u32)) -> Rect {
    Rect {
        left: logical_pos.0,
        top: logical_pos.1,
        right: logical_pos.0 + logical_size.0 as i32,
        bottom: logical_pos.1 + logical_size.1 as i32,
    }
}

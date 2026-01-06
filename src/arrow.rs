//! Arrow and redaction annotation module for drawing on screenshots

use image::RgbaImage;

use crate::screenshot::Rect;

/// Arrow annotation for drawing on screenshots
#[derive(Clone, Debug, PartialEq)]
pub struct ArrowAnnotation {
    /// Start point in global logical coordinates
    pub start_x: f32,
    pub start_y: f32,
    /// End point in global logical coordinates
    pub end_x: f32,
    pub end_y: f32,
}

/// Redaction annotation (black rectangle) for hiding sensitive content
#[derive(Clone, Debug, PartialEq)]
pub struct RedactAnnotation {
    /// Top-left point in global logical coordinates
    pub x: f32,
    pub y: f32,
    /// Bottom-right point in global logical coordinates
    pub x2: f32,
    pub y2: f32,
}

/// Draw arrows onto an image using the same geometry as the screen rendering
/// selection_rect: the selection rectangle in logical coordinates (used as origin)
/// scale: pixels per logical unit
pub fn draw_arrows_on_image(
    img: &mut RgbaImage,
    arrows: &[ArrowAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    let arrow_color = image::Rgba([230u8, 25u8, 25u8, 255u8]); // Red
    let border_color = image::Rgba([0u8, 0u8, 0u8, 255u8]); // Black
    let thickness = 4.0 * scale;
    let head_size = 16.0 * scale;
    // "Very thin" outline: ~1 physical pixel on 1x, ~2 on 2x, etc.
    let outline_px = (1.0 * scale).max(1.0);
    let border_thickness = thickness + 2.0 * outline_px;
    let border_head_size = head_size + outline_px;

    for arrow in arrows {
        // Convert from global logical to image pixel coordinates (float for precision)
        let start_x = (arrow.start_x - selection_rect.left as f32) * scale;
        let start_y = (arrow.start_y - selection_rect.top as f32) * scale;
        let end_x = (arrow.end_x - selection_rect.left as f32) * scale;
        let end_y = (arrow.end_y - selection_rect.top as f32) * scale;
        // Border first, then main arrow
        draw_single_arrow(
            img,
            start_x,
            start_y,
            end_x,
            end_y,
            border_color,
            border_thickness,
            border_head_size,
        );
        draw_single_arrow(
            img,
            start_x,
            start_y,
            end_x,
            end_y,
            arrow_color,
            thickness,
            head_size,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_single_arrow(
    img: &mut RgbaImage,
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    color: image::Rgba<u8>,
    thickness: f32,
    head_size: f32,
) {
    let dx = end_x - start_x;
    let dy = end_y - start_y;
    let length = (dx * dx + dy * dy).sqrt();
    if length < 5.0 {
        return;
    }

    // Normalize direction
    let nx = dx / length;
    let ny = dy / length;

    // Perpendicular vector for thickness
    let px = -ny * thickness / 2.0;
    let py = nx * thickness / 2.0;

    // Shaft end (before arrowhead)
    let shaft_end_x = end_x - nx * head_size;
    let shaft_end_y = end_y - ny * head_size;

    // Draw shaft as filled quadrilateral (rotated rectangle) - split into 2 triangles
    fill_triangle(
        img,
        start_x + px,
        start_y + py,
        start_x - px,
        start_y - py,
        shaft_end_x - px,
        shaft_end_y - py,
        color,
    );
    fill_triangle(
        img,
        start_x + px,
        start_y + py,
        shaft_end_x - px,
        shaft_end_y - py,
        shaft_end_x + px,
        shaft_end_y + py,
        color,
    );

    // Draw arrowhead as filled triangle
    let head_width = head_size * 0.5;
    let hpx = -ny * head_width;
    let hpy = nx * head_width;

    fill_triangle(
        img,
        shaft_end_x + hpx,
        shaft_end_y + hpy,
        shaft_end_x - hpx,
        shaft_end_y - hpy,
        end_x,
        end_y,
        color,
    );
}

/// Draw redaction rectangles onto an image
/// selection_rect: the selection rectangle in logical coordinates (used as origin)
/// scale: pixels per logical unit
pub fn draw_redactions_on_image(
    img: &mut RgbaImage,
    redactions: &[RedactAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    let redact_color = image::Rgba([0u8, 0u8, 0u8, 255u8]); // Black

    for redact in redactions {
        // Convert from global logical to image pixel coordinates
        let x1 = ((redact.x - selection_rect.left as f32) * scale).round() as i32;
        let y1 = ((redact.y - selection_rect.top as f32) * scale).round() as i32;
        let x2 = ((redact.x2 - selection_rect.left as f32) * scale).round() as i32;
        let y2 = ((redact.y2 - selection_rect.top as f32) * scale).round() as i32;

        // Normalize coordinates (ensure x1 < x2 and y1 < y2)
        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

        // Clamp to image bounds
        let min_x = min_x.max(0) as u32;
        let max_x = (max_x as u32).min(img.width().saturating_sub(1));
        let min_y = min_y.max(0) as u32;
        let max_y = (max_y as u32).min(img.height().saturating_sub(1));

        // Fill the rectangle
        for py in min_y..=max_y {
            for px in min_x..=max_x {
                img.put_pixel(px, py, redact_color);
            }
        }
    }
}

/// Fill a triangle using edge function rasterization
#[allow(clippy::too_many_arguments)]
fn fill_triangle(
    img: &mut RgbaImage,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    color: image::Rgba<u8>,
) {
    let (w, h) = (img.width() as i32, img.height() as i32);

    // Bounding box (no padding needed - we only fill inside pixels)
    let min_x = (x0.min(x1).min(x2).floor() as i32).max(0);
    let max_x = (x0.max(x1).max(x2).ceil() as i32).min(w - 1);
    let min_y = (y0.min(y1).min(y2).floor() as i32).max(0);
    let max_y = (y0.max(y1).max(y2).ceil() as i32).min(h - 1);

    // Signed area (2x) for barycentric coords
    let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
    if area.abs() < 0.001 {
        return; // Degenerate triangle
    }

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            // Simple MSAA (supersampling) to anti-alias triangle edges when burning into the final image.
            // This is important because saved/copied images come from CPU rasterization.
            const N: i32 = 4; // 4x4 = 16 samples per pixel
            let mut covered = 0;

            for sy in 0..N {
                for sx in 0..N {
                    // Subsample in pixel space
                    let x = px as f32 + (sx as f32 + 0.5) / N as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / N as f32;

                    // Edge functions (same sign = inside)
                    let e0 = (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0);
                    let e1 = (x2 - x1) * (y - y1) - (y2 - y1) * (x - x1);
                    let e2 = (x0 - x2) * (y - y2) - (y0 - y2) * (x - x2);

                    let inside = if area > 0.0 {
                        e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0
                    } else {
                        e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0
                    };

                    if inside {
                        covered += 1;
                    }
                }
            }

            if covered == 0 {
                continue;
            }

            let coverage = covered as f32 / (N * N) as f32;
            let src_a = (color.0[3] as f32) / 255.0;
            let alpha = (coverage * src_a).clamp(0.0, 1.0);

            if alpha >= 0.999 {
                img.put_pixel(px as u32, py as u32, color);
            } else {
                let dst = img.get_pixel(px as u32, py as u32);
                let inv = 1.0 - alpha;
                let r = (color.0[0] as f32 * alpha + dst.0[0] as f32 * inv).round() as u8;
                let g = (color.0[1] as f32 * alpha + dst.0[1] as f32 * inv).round() as u8;
                let b = (color.0[2] as f32 * alpha + dst.0[2] as f32 * inv).round() as u8;
                // Screenshot images are expected to be fully opaque.
                img.put_pixel(px as u32, py as u32, image::Rgba([r, g, b, 255]));
            }
        }
    }
}

//! Arrow and redaction annotation module for drawing on screenshots

use image::RgbaImage;

use crate::config::ShapeColor;
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
    /// Color of this arrow
    pub color: ShapeColor,
    /// Whether to draw shadow/border
    pub shadow: bool,
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

/// Pixelation annotation for obscuring sensitive content with pixelation effect
#[derive(Clone, Debug, PartialEq)]
pub struct PixelateAnnotation {
    /// Top-left point in global logical coordinates
    pub x: f32,
    pub y: f32,
    /// Bottom-right point in global logical coordinates
    pub x2: f32,
    pub y2: f32,
    /// Block size for this pixelation
    pub block_size: u32,
}


/// Outline rectangle annotation (no fill)
#[derive(Clone, Debug, PartialEq)]
pub struct RectOutlineAnnotation {
    /// Start point in global logical coordinates
    pub start_x: f32,
    pub start_y: f32,
    /// End point in global logical coordinates
    pub end_x: f32,
    pub end_y: f32,
    /// Color of this rectangle
    pub color: ShapeColor,
    /// Whether to draw shadow/border
    pub shadow: bool,
}

/// Outline circle/ellipse annotation (no fill)
#[derive(Clone, Debug, PartialEq)]
pub struct CircleOutlineAnnotation {
    /// Start point in global logical coordinates
    pub start_x: f32,
    pub start_y: f32,
    /// End point in global logical coordinates
    pub end_x: f32,
    pub end_y: f32,
    /// Color of this circle
    pub color: ShapeColor,
    /// Whether to draw shadow/border
    pub shadow: bool,
}

/// Unified annotation type for ordered drawing and undo/redo
#[derive(Clone, Debug, PartialEq)]
pub enum Annotation {
    Arrow(ArrowAnnotation),
    Circle(CircleOutlineAnnotation),
    Rectangle(RectOutlineAnnotation),
    Redact(RedactAnnotation),
    Pixelate(PixelateAnnotation),
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
    for arrow in arrows {
        let [r, g, b, a] = arrow.color.to_rgba_u8();
        let arrow_color = image::Rgba([r, g, b, a]);
        let border_color = image::Rgba([0u8, 0u8, 0u8, 255u8]); // Black
        let thickness = 4.0 * scale;
        let head_size = 16.0 * scale;
        // "Very thin" outline: ~1 physical pixel on 1x, ~2 on 2x, etc.
        let outline_px = (1.0 * scale).max(1.0);
        let border_thickness = thickness + 2.0 * outline_px;
        let border_head_size = head_size + outline_px;

        // Convert from global logical to image pixel coordinates (float for precision)
        let start_x = (arrow.start_x - selection_rect.left as f32) * scale;
        let start_y = (arrow.start_y - selection_rect.top as f32) * scale;
        let end_x = (arrow.end_x - selection_rect.left as f32) * scale;
        let end_y = (arrow.end_y - selection_rect.top as f32) * scale;

        // Border/shadow first, then main arrow
        if arrow.shadow {
            draw_single_arrow_lines(
                img,
                start_x,
                start_y,
                end_x,
                end_y,
                border_color,
                border_thickness,
                border_head_size,
            );
        }
        draw_single_arrow_lines(
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

/// Draw a single arrow using lines with rounded caps (not filled triangles)
#[allow(clippy::too_many_arguments)]
fn draw_single_arrow_lines(
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

    // Draw the shaft line from start to end
    draw_rounded_line(img, start_x, start_y, end_x, end_y, thickness, color);

    // Arrowhead: two angled lines at the tip
    let angle = 35.0_f32.to_radians(); // Angle of the arrowhead lines
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    // First head line (rotated clockwise from the arrow direction)
    let head1_dx = -nx * cos_a - (-ny) * sin_a;
    let head1_dy = -nx * sin_a + (-ny) * cos_a;
    let head1_end_x = end_x + head1_dx * head_size;
    let head1_end_y = end_y + head1_dy * head_size;
    draw_rounded_line(img, end_x, end_y, head1_end_x, head1_end_y, thickness, color);

    // Second head line (rotated counter-clockwise)
    let head2_dx = -nx * cos_a + (-ny) * sin_a;
    let head2_dy = -nx * (-sin_a) + (-ny) * cos_a;
    let head2_end_x = end_x + head2_dx * head_size;
    let head2_end_y = end_y + head2_dy * head_size;
    draw_rounded_line(img, end_x, end_y, head2_end_x, head2_end_y, thickness, color);
}

/// Draw a line with rounded end caps
fn draw_rounded_line(
    img: &mut RgbaImage,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
    color: image::Rgba<u8>,
) {
    // Draw the main line body
    draw_thick_line_aa(img, x0, y0, x1, y1, thickness, color);
    // Draw rounded caps at both ends
    let radius = thickness / 2.0;
    fill_circle_aa(img, x0, y0, radius, color);
    fill_circle_aa(img, x1, y1, radius, color);
}

/// Fill a circle with anti-aliasing (for rounded line caps)
fn fill_circle_aa(
    img: &mut RgbaImage,
    cx: f32,
    cy: f32,
    radius: f32,
    color: image::Rgba<u8>,
) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let r = radius.ceil() as i32 + 1;
    let min_x = (cx as i32 - r).max(0);
    let max_x = (cx as i32 + r).min(w - 1);
    let min_y = (cy as i32 - r).max(0);
    let max_y = (cy as i32 + r).min(h - 1);

    let r2 = radius * radius;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            // Supersampling for AA
            const N: i32 = 4;
            let mut covered = 0;

            for sy in 0..N {
                for sx in 0..N {
                    let x = px as f32 + (sx as f32 + 0.5) / N as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / N as f32;
                    let dx = x - cx;
                    let dy = y - cy;
                    if dx * dx + dy * dy <= r2 {
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
                img.put_pixel(px as u32, py as u32, image::Rgba([r, g, b, 255]));
            }
        }
    }
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

/// Draw pixelation rectangles onto an image
/// selection_rect: the selection rectangle in logical coordinates (used as origin)
/// scale: pixels per logical unit
pub fn draw_pixelations_on_image(
    img: &mut RgbaImage,
    pixelations: &[PixelateAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    for pixelate in pixelations {
        let block_size = pixelate.block_size.max(1); // Ensure at least 1 pixel blocks
        // Convert from global logical to image pixel coordinates
        let x1 = ((pixelate.x - selection_rect.left as f32) * scale).round() as i32;
        let y1 = ((pixelate.y - selection_rect.top as f32) * scale).round() as i32;
        let x2 = ((pixelate.x2 - selection_rect.left as f32) * scale).round() as i32;
        let y2 = ((pixelate.y2 - selection_rect.top as f32) * scale).round() as i32;

        // Normalize coordinates (ensure x1 < x2 and y1 < y2)
        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

        // Clamp to image bounds
        let min_x = min_x.max(0) as u32;
        let max_x = (max_x as u32).min(img.width().saturating_sub(1));
        let min_y = min_y.max(0) as u32;
        let max_y = (max_y as u32).min(img.height().saturating_sub(1));

        // Process each block
        let mut block_y = min_y;
        while block_y <= max_y {
            let block_end_y = (block_y + block_size - 1).min(max_y);

            let mut block_x = min_x;
            while block_x <= max_x {
                let block_end_x = (block_x + block_size - 1).min(max_x);

                // Calculate average color for this block
                let mut total_r: u64 = 0;
                let mut total_g: u64 = 0;
                let mut total_b: u64 = 0;
                let mut total_a: u64 = 0;
                let mut pixel_count: u64 = 0;

                for py in block_y..=block_end_y {
                    for px in block_x..=block_end_x {
                        let pixel = img.get_pixel(px, py);
                        total_r += pixel[0] as u64;
                        total_g += pixel[1] as u64;
                        total_b += pixel[2] as u64;
                        total_a += pixel[3] as u64;
                        pixel_count += 1;
                    }
                }

                if pixel_count > 0 {
                    let avg_color = image::Rgba([
                        (total_r / pixel_count) as u8,
                        (total_g / pixel_count) as u8,
                        (total_b / pixel_count) as u8,
                        (total_a / pixel_count) as u8,
                    ]);

                    // Fill the block with the average color
                    for py in block_y..=block_end_y {
                        for px in block_x..=block_end_x {
                            img.put_pixel(px, py, avg_color);
                        }
                    }
                }

                block_x += block_size;
            }
            block_y += block_size;
        }
    }
}

/// Draw rectangle outlines onto an image (colored stroke, no fill)
pub fn draw_rect_outlines_on_image(
    img: &mut RgbaImage,
    rects: &[RectOutlineAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    let border_color = image::Rgba([0u8, 0u8, 0u8, 255u8]);
    let thickness = (3.0 * scale).max(1.0);
    let border_thickness = (5.0 * scale).max(2.0);

    for rect in rects {
        let [r, g, b, a] = rect.color.to_rgba_u8();
        let color = image::Rgba([r, g, b, a]);

        // Convert to pixel coords
        let x1 = (rect.start_x - selection_rect.left as f32) * scale;
        let y1 = (rect.start_y - selection_rect.top as f32) * scale;
        let x2 = (rect.end_x - selection_rect.left as f32) * scale;
        let y2 = (rect.end_y - selection_rect.top as f32) * scale;

        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

        // Shadow/border first
        if rect.shadow {
            draw_thick_line_aa(img, min_x, min_y, max_x, min_y, border_thickness, border_color);
            draw_thick_line_aa(img, max_x, min_y, max_x, max_y, border_thickness, border_color);
            draw_thick_line_aa(img, max_x, max_y, min_x, max_y, border_thickness, border_color);
            draw_thick_line_aa(img, min_x, max_y, min_x, min_y, border_thickness, border_color);
        }

        // 4 sides
        draw_thick_line_aa(img, min_x, min_y, max_x, min_y, thickness, color);
        draw_thick_line_aa(img, max_x, min_y, max_x, max_y, thickness, color);
        draw_thick_line_aa(img, max_x, max_y, min_x, max_y, thickness, color);
        draw_thick_line_aa(img, min_x, max_y, min_x, min_y, thickness, color);
    }
}

/// Draw circle/ellipse outlines onto an image (colored stroke, no fill)
pub fn draw_circle_outlines_on_image(
    img: &mut RgbaImage,
    circles: &[CircleOutlineAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    let border_color = image::Rgba([0u8, 0u8, 0u8, 255u8]);
    let thickness = (3.0 * scale).max(1.0);
    let border_thickness = (5.0 * scale).max(2.0);

    for c in circles {
        let [r, g, b, a] = c.color.to_rgba_u8();
        let color = image::Rgba([r, g, b, a]);

        let x1 = (c.start_x - selection_rect.left as f32) * scale;
        let y1 = (c.start_y - selection_rect.top as f32) * scale;
        let x2 = (c.end_x - selection_rect.left as f32) * scale;
        let y2 = (c.end_y - selection_rect.top as f32) * scale;

        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let rx = ((max_x - min_x) * 0.5).max(1.0);
        let ry = ((max_y - min_y) * 0.5).max(1.0);

        // Adaptive segment count: more segments for larger circles
        let approx_r = rx.max(ry);
        let segments = ((approx_r * 0.35).clamp(24.0, 96.0)) as usize;
        let step = std::f32::consts::TAU / segments as f32;

        // Draw shadow/border first
        if c.shadow {
            let mut prev_x = cx + rx;
            let mut prev_y = cy;
            for i in 1..=segments {
                let t = i as f32 * step;
                let x = cx + rx * t.cos();
                let y = cy + ry * t.sin();
                draw_thick_line_aa(img, prev_x, prev_y, x, y, border_thickness, border_color);
                prev_x = x;
                prev_y = y;
            }
        }

        // Draw main color
        let mut prev_x = cx + rx;
        let mut prev_y = cy;
        for i in 1..=segments {
            let t = i as f32 * step;
            let x = cx + rx * t.cos();
            let y = cy + ry * t.sin();
            draw_thick_line_aa(img, prev_x, prev_y, x, y, thickness, color);
            prev_x = x;
            prev_y = y;
        }
    }
}

fn draw_thick_line_aa(
    img: &mut RgbaImage,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
    color: image::Rgba<u8>,
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.01 {
        return;
    }
    let nx = dx / len;
    let ny = dy / len;
    let px = -ny * thickness / 2.0;
    let py = nx * thickness / 2.0;

    // Quad as 2 triangles
    fill_triangle(img, x0 + px, y0 + py, x0 - px, y0 - py, x1 - px, y1 - py, color);
    fill_triangle(img, x0 + px, y0 + py, x1 - px, y1 - py, x1 + px, y1 + py, color);
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

/// Draw all annotations in order (for proper layering and undo/redo support)
/// Redactions and pixelations are ALWAYS drawn first (in their relative order),
/// then annotations (arrows, circles, rectangles) are drawn on top (in their relative order).
/// This ensures annotations are never obscured by redactions.
pub fn draw_annotations_in_order(
    img: &mut RgbaImage,
    annotations: &[Annotation],
    selection_rect: &Rect,
    scale: f32,
) {
    // First pass: draw all redactions and pixelations (in order)
    for annotation in annotations {
        match annotation {
            Annotation::Redact(redact) => {
                draw_redactions_on_image(img, &[redact.clone()], selection_rect, scale);
            }
            Annotation::Pixelate(pixelate) => {
                draw_pixelations_on_image(img, &[pixelate.clone()], selection_rect, scale);
            }
            _ => {}
        }
    }
    
    // Second pass: draw all shape annotations on top (in order)
    for annotation in annotations {
        match annotation {
            Annotation::Arrow(arrow) => {
                draw_arrows_on_image(img, &[arrow.clone()], selection_rect, scale);
            }
            Annotation::Circle(circle) => {
                draw_circle_outlines_on_image(img, &[circle.clone()], selection_rect, scale);
            }
            Annotation::Rectangle(rect) => {
                draw_rect_outlines_on_image(img, &[rect.clone()], selection_rect, scale);
            }
            _ => {}
        }
    }
}

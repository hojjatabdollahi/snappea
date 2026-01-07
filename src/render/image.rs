//! Image rendering for annotations using tiny-skia
//!
//! These functions draw annotations onto RgbaImage for saving to disk.

use image::RgbaImage;
use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::geometry::{self, arrow, shape};
use crate::domain::{
    Annotation, ArrowAnnotation, CircleOutlineAnnotation, PixelateAnnotation, Rect,
    RectOutlineAnnotation, RedactAnnotation,
};

/// Convert RgbaImage to Pixmap, apply drawing function, and copy back
fn with_pixmap(img: &mut RgbaImage, f: impl FnOnce(&mut Pixmap)) {
    let (w, h) = (img.width(), img.height());
    let Some(mut pixmap) = Pixmap::from_vec(
        img.as_raw().clone(),
        tiny_skia::IntSize::from_wh(w, h).unwrap(),
    ) else {
        return;
    };

    f(&mut pixmap);

    // Copy back
    img.copy_from_slice(pixmap.data());
}

/// Build an arrow path as stroked lines (shaft + two angled head lines)
fn build_arrow_path(
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    head_size: f32,
) -> Option<tiny_skia::Path> {
    let (head1_x, head1_y, head2_x, head2_y) =
        arrow::head_points(start_x, start_y, end_x, end_y, head_size)?;

    let mut pb = PathBuilder::new();

    // Shaft line from start to end
    pb.move_to(start_x, start_y);
    pb.line_to(end_x, end_y);

    // First head line
    pb.move_to(end_x, end_y);
    pb.line_to(head1_x, head1_y);

    // Second head line
    pb.move_to(end_x, end_y);
    pb.line_to(head2_x, head2_y);

    pb.finish()
}

/// Build an ellipse path using cubic bezier curves
fn build_ellipse_path(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<tiny_skia::Path> {
    let kx = rx * shape::BEZIER_K;
    let ky = ry * shape::BEZIER_K;

    let mut pb = PathBuilder::new();

    // Start at top
    pb.move_to(cx, cy - ry);

    // Top to right
    pb.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);

    // Right to bottom
    pb.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);

    // Bottom to left
    pb.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);

    // Left to top
    pb.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);

    pb.close();
    pb.finish()
}

/// Draw arrows onto an image using tiny-skia with stroked lines and rounded caps
pub fn draw_arrows_on_image(
    img: &mut RgbaImage,
    arrows: &[ArrowAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    if arrows.is_empty() {
        return;
    }

    with_pixmap(img, |pixmap| {
        for arrow_ann in arrows {
            let [r, g, b, a] = arrow_ann.color.to_rgba_u8();

            // Convert from global logical to image pixel coordinates
            let start_x = (arrow_ann.start_x - selection_rect.left as f32) * scale;
            let start_y = (arrow_ann.start_y - selection_rect.top as f32) * scale;
            let end_x = (arrow_ann.end_x - selection_rect.left as f32) * scale;
            let end_y = (arrow_ann.end_y - selection_rect.top as f32) * scale;

            let thickness = arrow::THICKNESS * scale;
            let head_size = arrow::HEAD_SIZE * scale;
            let outline = arrow::OUTLINE * scale;

            // Draw shadow/border first (thicker stroke)
            if arrow_ann.shadow
                && let Some(path) =
                    build_arrow_path(start_x, start_y, end_x, end_y, head_size + outline)
            {
                let mut paint = Paint::default();
                paint.set_color_rgba8(0, 0, 0, 220);
                paint.anti_alias = true;

                let stroke = Stroke {
                    width: thickness + outline * 2.0,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Default::default()
                };
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }

            // Draw main arrow with rounded caps
            if let Some(path) = build_arrow_path(start_x, start_y, end_x, end_y, head_size) {
                let mut paint = Paint::default();
                paint.set_color_rgba8(r, g, b, a);
                paint.anti_alias = true;

                let stroke = Stroke {
                    width: thickness,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Default::default()
                };
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }
        }
    });
}

/// Draw redaction rectangles onto an image
pub fn draw_redactions_on_image(
    img: &mut RgbaImage,
    redactions: &[RedactAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    if redactions.is_empty() {
        return;
    }

    with_pixmap(img, |pixmap| {
        let mut paint = Paint::default();
        paint.set_color(Color::BLACK);

        for redact in redactions {
            let x1 = (redact.x - selection_rect.left as f32) * scale;
            let y1 = (redact.y - selection_rect.top as f32) * scale;
            let x2 = (redact.x2 - selection_rect.left as f32) * scale;
            let y2 = (redact.y2 - selection_rect.top as f32) * scale;

            let (min_x, min_y, max_x, max_y) = geometry::normalize_rect(x1, y1, x2, y2);

            if let Some(rect) =
                tiny_skia::Rect::from_xywh(min_x, min_y, max_x - min_x, max_y - min_y)
            {
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            }
        }
    });
}

/// Draw pixelation rectangles onto an image
pub fn draw_pixelations_on_image(
    img: &mut RgbaImage,
    pixelations: &[PixelateAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    for pixelate in pixelations {
        // Scale the block size from display pixels to image pixels
        let block_size = ((pixelate.block_size as f32) * scale).round().max(1.0) as u32;
        let x1 = ((pixelate.x - selection_rect.left as f32) * scale).round() as i32;
        let y1 = ((pixelate.y - selection_rect.top as f32) * scale).round() as i32;
        let x2 = ((pixelate.x2 - selection_rect.left as f32) * scale).round() as i32;
        let y2 = ((pixelate.y2 - selection_rect.top as f32) * scale).round() as i32;

        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

        let min_x = min_x.max(0) as u32;
        let max_x = (max_x as u32).min(img.width().saturating_sub(1));
        let min_y = min_y.max(0) as u32;
        let max_y = (max_y as u32).min(img.height().saturating_sub(1));

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

/// Draw rectangle outlines onto an image using tiny-skia strokes
pub fn draw_rect_outlines_on_image(
    img: &mut RgbaImage,
    rects: &[RectOutlineAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    if rects.is_empty() {
        return;
    }

    with_pixmap(img, |pixmap| {
        let thickness = (shape::THICKNESS * scale).max(1.0);
        let border_thickness = (shape::BORDER_THICKNESS * scale).max(2.0);

        for rect in rects {
            let [r, g, b, a] = rect.color.to_rgba_u8();

            let x1 = (rect.start_x - selection_rect.left as f32) * scale;
            let y1 = (rect.start_y - selection_rect.top as f32) * scale;
            let x2 = (rect.end_x - selection_rect.left as f32) * scale;
            let y2 = (rect.end_y - selection_rect.top as f32) * scale;

            let (min_x, min_y, max_x, max_y) = geometry::normalize_rect(x1, y1, x2, y2);

            // Build rectangle path
            let mut pb = PathBuilder::new();
            pb.move_to(min_x, min_y);
            pb.line_to(max_x, min_y);
            pb.line_to(max_x, max_y);
            pb.line_to(min_x, max_y);
            pb.close();
            let Some(path) = pb.finish() else {
                continue;
            };

            // Draw shadow first
            if rect.shadow {
                let mut paint = Paint::default();
                paint.set_color_rgba8(0, 0, 0, 220);
                paint.anti_alias = true;

                let stroke = Stroke {
                    width: border_thickness,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Default::default()
                };
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }

            // Draw main stroke
            let mut paint = Paint::default();
            paint.set_color_rgba8(r, g, b, a);
            paint.anti_alias = true;

            let stroke = Stroke {
                width: thickness,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    });
}

/// Draw circle/ellipse outlines onto an image using tiny-skia
pub fn draw_circle_outlines_on_image(
    img: &mut RgbaImage,
    circles: &[CircleOutlineAnnotation],
    selection_rect: &Rect,
    scale: f32,
) {
    if circles.is_empty() {
        return;
    }

    with_pixmap(img, |pixmap| {
        let thickness = (shape::THICKNESS * scale).max(1.0);
        let border_thickness = (shape::BORDER_THICKNESS * scale).max(2.0);

        for c in circles {
            let [r, g, b, a] = c.color.to_rgba_u8();

            let x1 = (c.start_x - selection_rect.left as f32) * scale;
            let y1 = (c.start_y - selection_rect.top as f32) * scale;
            let x2 = (c.end_x - selection_rect.left as f32) * scale;
            let y2 = (c.end_y - selection_rect.top as f32) * scale;

            let (min_x, min_y, max_x, max_y) = geometry::normalize_rect(x1, y1, x2, y2);
            let (cx, cy, rx, ry) = geometry::ellipse_from_bounds(min_x, min_y, max_x, max_y);

            // Build ellipse path using bezier curves
            let Some(path) = build_ellipse_path(cx, cy, rx, ry) else {
                continue;
            };

            // Draw shadow first
            if c.shadow {
                let mut paint = Paint::default();
                paint.set_color_rgba8(0, 0, 0, 220);
                paint.anti_alias = true;

                let stroke = Stroke {
                    width: border_thickness,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Default::default()
                };
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }

            // Draw main stroke
            let mut paint = Paint::default();
            paint.set_color_rgba8(r, g, b, a);
            paint.anti_alias = true;

            let stroke = Stroke {
                width: thickness,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    });
}

/// Draw all annotations in order (for proper layering and undo/redo support)
///
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
                draw_redactions_on_image(img, std::slice::from_ref(redact), selection_rect, scale);
            }
            Annotation::Pixelate(pixelate) => {
                draw_pixelations_on_image(
                    img,
                    std::slice::from_ref(pixelate),
                    selection_rect,
                    scale,
                );
            }
            _ => {}
        }
    }

    // Second pass: draw all shape annotations on top (in order)
    for annotation in annotations {
        match annotation {
            Annotation::Arrow(arrow) => {
                draw_arrows_on_image(img, std::slice::from_ref(arrow), selection_rect, scale);
            }
            Annotation::Circle(circle) => {
                draw_circle_outlines_on_image(
                    img,
                    std::slice::from_ref(circle),
                    selection_rect,
                    scale,
                );
            }
            Annotation::Rectangle(rect) => {
                draw_rect_outlines_on_image(img, std::slice::from_ref(rect), selection_rect, scale);
            }
            _ => {}
        }
    }
}

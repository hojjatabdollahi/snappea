//! Redaction and pixelation overlay drawing functions
//!
//! This module contains helper functions for drawing redactions and pixelations
//! on the screenshot preview.

use cosmic::iced::Color;
use cosmic::iced_core::{Background, Border, Rectangle};
use image::RgbaImage;

use crate::domain::{Annotation, PixelateAnnotation, RedactAnnotation};

/// Image source for pixelation sampling
pub enum PixelationSource<'a> {
    /// Regular mode: sample from screenshot image with scale factor
    Screenshot { image: &'a RgbaImage, scale: f32 },
    /// Window mode: sample from window image with offset and scale
    Window {
        image: &'a RgbaImage,
        offset: (f32, f32), // (win_x, win_y)
        scale: f32,         // display_to_img_scale
    },
}

/// Draw a single redaction rectangle
pub fn draw_redaction(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    redact: &RedactAnnotation,
    output_offset: (f32, f32),
) {
    use cosmic::iced_core::Renderer;

    let (offset_x, offset_y) = output_offset;
    let x1 = redact.x - offset_x;
    let y1 = redact.y - offset_y;
    let x2 = redact.x2 - offset_x;
    let y2 = redact.y2 - offset_y;
    let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
    let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

    let rect = Rectangle {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    };

    renderer.with_layer(*viewport, |renderer| {
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: rect,
                border: Border::default(),
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(Color::BLACK),
        );
    });
}

/// Draw a single pixelation region
#[allow(clippy::too_many_arguments)]
pub fn draw_pixelation(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    pixelate: &PixelateAnnotation,
    output_offset: (f32, f32),
    source: &PixelationSource,
) {
    let (offset_x, offset_y) = output_offset;
    let x1 = pixelate.x - offset_x;
    let y1 = pixelate.y - offset_y;
    let x2 = pixelate.x2 - offset_x;
    let y2 = pixelate.y2 - offset_y;
    let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
    let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

    match source {
        PixelationSource::Window {
            image,
            offset,
            scale,
        } => {
            let (win_x, win_y) = *offset;
            let block_size_display = pixelate.block_size as f32;
            draw_pixelation_blocks_window(
                renderer,
                viewport,
                image,
                min_x,
                min_y,
                max_x,
                max_y,
                win_x,
                win_y,
                *scale,
                block_size_display,
            );
        }
        PixelationSource::Screenshot { image, scale } => {
            let block_size_logical = pixelate.block_size as f32 / *scale;
            draw_pixelation_blocks_screenshot(
                renderer,
                viewport,
                image,
                min_x,
                min_y,
                max_x,
                max_y,
                *scale,
                block_size_logical,
            );
        }
    }
}

/// Draw all redactions and pixelations from annotations array
pub fn draw_redactions_and_pixelations(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    annotations: &[Annotation],
    output_offset: (f32, f32),
    source: &PixelationSource,
) {
    for annotation in annotations {
        match annotation {
            Annotation::Redact(redact) => {
                draw_redaction(renderer, viewport, redact, output_offset);
            }
            Annotation::Pixelate(pixelate) => {
                draw_pixelation(renderer, viewport, pixelate, output_offset, source);
            }
            _ => {}
        }
    }
}

/// Draw pixelation preview (while dragging)
#[allow(clippy::too_many_arguments)]
pub fn draw_pixelation_preview(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    start: (f32, f32), // Global start position
    end: (f32, f32),   // Local end position (cursor)
    output_offset: (f32, f32),
    block_size: u32,
    source: &PixelationSource,
) {
    use cosmic::iced_core::Renderer;

    let (offset_x, offset_y) = output_offset;
    let local_start_x = start.0 - offset_x;
    let local_start_y = start.1 - offset_y;
    let (end_x, end_y) = end;

    let (min_x, max_x) = if local_start_x < end_x {
        (local_start_x, end_x)
    } else {
        (end_x, local_start_x)
    };
    let (min_y, max_y) = if local_start_y < end_y {
        (local_start_y, end_y)
    } else {
        (end_y, local_start_y)
    };

    match source {
        PixelationSource::Window {
            image,
            offset,
            scale,
        } => {
            let (win_x, win_y) = *offset;
            let block_size_display = block_size as f32;
            draw_pixelation_blocks_window(
                renderer,
                viewport,
                image,
                min_x,
                min_y,
                max_x,
                max_y,
                win_x,
                win_y,
                *scale,
                block_size_display,
            );
        }
        PixelationSource::Screenshot { image, scale } => {
            let block_size_logical = block_size as f32 / *scale;
            draw_pixelation_blocks_screenshot(
                renderer,
                viewport,
                image,
                min_x,
                min_y,
                max_x,
                max_y,
                *scale,
                block_size_logical,
            );
        }
    }

    // Draw border
    renderer.with_layer(*viewport, |renderer| {
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: Rectangle {
                    x: min_x,
                    y: min_y,
                    width: max_x - min_x,
                    height: max_y - min_y,
                },
                border: Border {
                    color: Color::WHITE,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(Color::TRANSPARENT),
        );
    });
}

/// Draw redaction preview (while dragging)
pub fn draw_redaction_preview(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    start: (f32, f32), // Global start position
    end: (f32, f32),   // Local end position (cursor)
    output_offset: (f32, f32),
) {
    use cosmic::iced_core::Renderer;

    let (offset_x, offset_y) = output_offset;
    let local_start_x = start.0 - offset_x;
    let local_start_y = start.1 - offset_y;
    let (end_x, end_y) = end;

    let (min_x, max_x) = if local_start_x < end_x {
        (local_start_x, end_x)
    } else {
        (end_x, local_start_x)
    };
    let (min_y, max_y) = if local_start_y < end_y {
        (local_start_y, end_y)
    } else {
        (end_y, local_start_y)
    };

    renderer.with_layer(*viewport, |renderer| {
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: Rectangle {
                    x: min_x,
                    y: min_y,
                    width: max_x - min_x,
                    height: max_y - min_y,
                },
                border: Border {
                    color: Color::WHITE,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.7)),
        );
    });
}

// ============ Internal helper functions ============

/// Draw pixelation blocks sampling from window image
#[allow(clippy::too_many_arguments)]
fn draw_pixelation_blocks_window(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    image: &RgbaImage,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    win_x: f32,
    win_y: f32,
    display_to_img_scale: f32,
    block_size: f32,
) {
    use cosmic::iced_core::Renderer;

    renderer.with_layer(*viewport, |renderer| {
        let mut y = min_y;
        while y < max_y {
            let mut x = min_x;
            let block_h = block_size.min(max_y - y);
            while x < max_x {
                let block_w = block_size.min(max_x - x);

                // Convert from screen coords to window image coords
                let win_rel_x = x - win_x;
                let win_rel_y = y - win_y;
                let img_x = (win_rel_x * display_to_img_scale).round() as i32;
                let img_y = (win_rel_y * display_to_img_scale).round() as i32;
                let img_x2 = ((win_rel_x + block_w) * display_to_img_scale).round() as i32;
                let img_y2 = ((win_rel_y + block_h) * display_to_img_scale).round() as i32;

                // Skip if outside window image bounds
                if img_x >= 0
                    && img_y >= 0
                    && img_x2 > 0
                    && img_y2 > 0
                    && let Some(color) = sample_average_color(
                        image,
                        img_x as u32,
                        img_y as u32,
                        img_x2 as u32,
                        img_y2 as u32,
                    )
                {
                    renderer.fill_quad(
                        cosmic::iced_core::renderer::Quad {
                            bounds: Rectangle {
                                x,
                                y,
                                width: block_w,
                                height: block_h,
                            },
                            border: Border::default(),
                            shadow: cosmic::iced_core::Shadow::default(),
                        },
                        Background::Color(color),
                    );
                }
                x += block_w;
            }
            y += block_h;
        }
    });
}

/// Draw pixelation blocks sampling from screenshot image
#[allow(clippy::too_many_arguments)]
fn draw_pixelation_blocks_screenshot(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    image: &RgbaImage,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    image_scale: f32,
    block_size: f32,
) {
    use cosmic::iced_core::Renderer;

    renderer.with_layer(*viewport, |renderer| {
        let mut y = min_y;
        while y < max_y {
            let mut x = min_x;
            let block_h = block_size.min(max_y - y);
            while x < max_x {
                let block_w = block_size.min(max_x - x);

                let img_x = (x * image_scale).round() as u32;
                let img_y = (y * image_scale).round() as u32;
                let img_x2 = ((x + block_w) * image_scale).round() as u32;
                let img_y2 = ((y + block_h) * image_scale).round() as u32;

                if let Some(color) = sample_average_color(image, img_x, img_y, img_x2, img_y2) {
                    renderer.fill_quad(
                        cosmic::iced_core::renderer::Quad {
                            bounds: Rectangle {
                                x,
                                y,
                                width: block_w,
                                height: block_h,
                            },
                            border: Border::default(),
                            shadow: cosmic::iced_core::Shadow::default(),
                        },
                        Background::Color(color),
                    );
                }
                x += block_w;
            }
            y += block_h;
        }
    });
}

/// Sample the average color from a region of an image
fn sample_average_color(image: &RgbaImage, x1: u32, y1: u32, x2: u32, y2: u32) -> Option<Color> {
    let img_x = x1.min(image.width().saturating_sub(1));
    let img_y = y1.min(image.height().saturating_sub(1));
    let img_x2 = x2.min(image.width());
    let img_y2 = y2.min(image.height());

    let mut total_r: u64 = 0;
    let mut total_g: u64 = 0;
    let mut total_b: u64 = 0;
    let mut pixel_count: u64 = 0;

    for py in img_y..img_y2 {
        for px in img_x..img_x2 {
            let pixel = image.get_pixel(px, py);
            total_r += pixel[0] as u64;
            total_g += pixel[1] as u64;
            total_b += pixel[2] as u64;
            pixel_count += 1;
        }
    }

    if pixel_count > 0 {
        Some(Color::from_rgb8(
            (total_r / pixel_count) as u8,
            (total_g / pixel_count) as u8,
            (total_b / pixel_count) as u8,
        ))
    } else {
        None
    }
}

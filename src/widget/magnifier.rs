//! Magnifier widget for zoomed preview during selection

use cosmic::iced::Color;
use cosmic::iced_core::{Border, Point, Rectangle, Shadow, Size, renderer::Quad};
use image::RgbaImage;

/// Magnifier radius in pixels
pub const MAGNIFIER_RADIUS: f32 = 60.0;
/// Magnifier zoom factor
pub const MAGNIFIER_ZOOM: f32 = 2.5;

/// Draw a magnifier showing a zoomed view of the image at the specified position
/// 
/// # Arguments
/// * `renderer` - The renderer to draw with
/// * `screenshot_image` - The source image to sample from
/// * `image_scale` - Scale factor for converting logical to image coordinates
/// * `drag_x`, `drag_y` - Position being magnified (in global logical coords)
/// * `output_rect` - The output rectangle (for coordinate conversion)
/// * `outer_size` - The outer size bounds for positioning
/// * `outer_rect` - The outer rectangle for coordinate conversion  
/// * `accent` - Accent color for the border
pub fn draw_magnifier(
    renderer: &mut cosmic::Renderer,
    screenshot_image: &RgbaImage,
    image_scale: f32,
    drag_x: i32,
    drag_y: i32,
    output_rect: &crate::screenshot::Rect,
    outer_size: Size,
    outer_rect: Rectangle,
    accent: Color,
) {
    use cosmic::iced_core::Renderer as _;
    
    renderer.with_layer(Rectangle::new(Point::ORIGIN, outer_size), |renderer| {
        // Convert to widget-local coordinates
        let cursor_pos = Point::new(drag_x as f32 - outer_rect.x, drag_y as f32 - outer_rect.y);

        // Position magnifier offset from cursor
        let magnifier_offset = MAGNIFIER_RADIUS + 20.0;
        let mag_center_x = (cursor_pos.x + magnifier_offset)
            .min(outer_size.width - MAGNIFIER_RADIUS - 5.0);
        let mag_center_y =
            (cursor_pos.y - magnifier_offset).max(MAGNIFIER_RADIUS + 5.0);

        // Sample from the screenshot image at the drag position
        // Convert to image coordinates
        let img_x = ((drag_x - output_rect.left) as f32 * image_scale) as i32;
        let img_y = ((drag_y - output_rect.top) as f32 * image_scale) as i32;

        let img_width = screenshot_image.width() as i32;
        let img_height = screenshot_image.height() as i32;

        // Draw magnifier background (dark circle, no border yet)
        let mag_bounds = Rectangle::new(
            Point::new(
                mag_center_x - MAGNIFIER_RADIUS,
                mag_center_y - MAGNIFIER_RADIUS,
            ),
            Size::new(MAGNIFIER_RADIUS * 2.0, MAGNIFIER_RADIUS * 2.0),
        );

        renderer.fill_quad(
            Quad {
                bounds: mag_bounds,
                border: Border {
                    radius: MAGNIFIER_RADIUS.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
            },
            Color::from_rgba(0.0, 0.0, 0.0, 0.9),
        );

        // Draw pixels inside magnifier
        let pixels_to_draw = (MAGNIFIER_RADIUS * 2.0 / MAGNIFIER_ZOOM) as i32;
        for dy in -pixels_to_draw / 2..=pixels_to_draw / 2 {
            for dx in -pixels_to_draw / 2..=pixels_to_draw / 2 {
                let src_x = img_x + dx;
                let src_y = img_y + dy;

                // Check if within magnifier circle (with margin for border)
                let dist = ((dx * dx + dy * dy) as f32).sqrt() * MAGNIFIER_ZOOM;
                if dist > MAGNIFIER_RADIUS - 3.0 {
                    continue;
                }

                // Sample pixel if in bounds
                if src_x >= 0 && src_x < img_width && src_y >= 0 && src_y < img_height {
                    let pixel = screenshot_image.get_pixel(src_x as u32, src_y as u32);
                    let color = Color::from_rgba8(
                        pixel[0],
                        pixel[1],
                        pixel[2],
                        pixel[3] as f32 / 255.0,
                    );

                    // Calculate position in magnifier
                    let mag_px = mag_center_x + dx as f32 * MAGNIFIER_ZOOM;
                    let mag_py = mag_center_y + dy as f32 * MAGNIFIER_ZOOM;

                    // Draw zoomed pixel
                    let pixel_bounds = Rectangle::new(
                        Point::new(
                            mag_px - MAGNIFIER_ZOOM / 2.0,
                            mag_py - MAGNIFIER_ZOOM / 2.0,
                        ),
                        Size::new(MAGNIFIER_ZOOM, MAGNIFIER_ZOOM),
                    );

                    renderer.fill_quad(
                        Quad {
                            bounds: pixel_bounds,
                            border: Border::default(),
                            shadow: Shadow::default(),
                        },
                        color,
                    );
                }
            }
        }

        // Draw crosshair in center
        let crosshair_size = 8.0;
        let crosshair_color = Color::from_rgba(1.0, 1.0, 1.0, 0.8);

        // Horizontal line
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(mag_center_x - crosshair_size, mag_center_y - 0.5),
                    Size::new(crosshair_size * 2.0, 1.0),
                ),
                border: Border::default(),
                shadow: Shadow::default(),
            },
            crosshair_color,
        );

        // Vertical line
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(mag_center_x - 0.5, mag_center_y - crosshair_size),
                    Size::new(1.0, crosshair_size * 2.0),
                ),
                border: Border::default(),
                shadow: Shadow::default(),
            },
            crosshair_color,
        );

        // Draw border on top of everything
        renderer.fill_quad(
            Quad {
                bounds: mag_bounds,
                border: Border {
                    radius: MAGNIFIER_RADIUS.into(),
                    width: 2.0,
                    color: accent,
                },
                shadow: Shadow::default(),
            },
            Color::TRANSPARENT,
        );
    });
}

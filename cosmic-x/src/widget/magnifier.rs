// SPDX-License-Identifier: GPL-3.0-only

//! Loupe under the pointer while dragging a selection.

use cosmic::iced::Color;
use cosmic::iced::core::image as iced_image;
use cosmic::iced::core::{Border, Point, Radians, Rectangle, Shadow, Size, renderer::Quad};

/// Magnifier radius in pixels
pub const MAGNIFIER_RADIUS: f32 = 60.0;
/// Magnifier zoom factor
pub const MAGNIFIER_ZOOM: f32 = 2.5;

/// Draw a loupe centered on `(drag_x, drag_y)` from the screenshot's own texture,
/// scaled up and clipped to a circle, so the pixel under the pointer sits at the
/// crosshair. Nothing is copied, since the compositor already holds the image.
#[allow(clippy::too_many_arguments)]
pub fn draw_magnifier(
    renderer: &mut cosmic::Renderer,
    screenshot: &iced_image::Handle,
    image_size: (u32, u32),
    image_scale: f32,
    drag_x: i32,
    drag_y: i32,
    output_rect: &crate::geometry::Rect,
    outer_size: Size,
    outer_rect: Rectangle,
    accent: Color,
) {
    use cosmic::iced::core::Renderer as _;
    use cosmic::iced::core::image::Renderer as _;

    renderer.with_layer(Rectangle::new(Point::ORIGIN, outer_size), |renderer| {
        // Widget-local center: the point being dragged.
        let center = Point::new(drag_x as f32 - outer_rect.x, drag_y as f32 - outer_rect.y);
        let mag_bounds = Rectangle::new(
            Point::new(center.x - MAGNIFIER_RADIUS, center.y - MAGNIFIER_RADIUS),
            Size::new(MAGNIFIER_RADIUS * 2.0, MAGNIFIER_RADIUS * 2.0),
        );

        // Dark disc underneath, seen where the loupe reaches past the screenshot.
        renderer.fill_quad(
            Quad {
                bounds: mag_bounds,
                border: Border {
                    radius: MAGNIFIER_RADIUS.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Color::from_rgba(0.0, 0.0, 0.0, 0.9),
        );

        // The whole screenshot at `MAGNIFIER_ZOOM` logical pixels per image pixel,
        // placed so the dragged point lands on the center. The clip shows the disc.
        let px = (drag_x - output_rect.left) as f32 * image_scale;
        let py = (drag_y - output_rect.top) as f32 * image_scale;
        let image_bounds = Rectangle::new(
            Point::new(
                px.mul_add(-MAGNIFIER_ZOOM, center.x),
                py.mul_add(-MAGNIFIER_ZOOM, center.y),
            ),
            Size::new(
                image_size.0 as f32 * MAGNIFIER_ZOOM,
                image_size.1 as f32 * MAGNIFIER_ZOOM,
            ),
        );
        renderer.draw_image(
            iced_image::Image {
                handle: screenshot.clone(),
                filter_method: iced_image::FilterMethod::Nearest,
                rotation: Radians(0.0),
                border_radius: MAGNIFIER_RADIUS.into(),
                opacity: 1.0,
                snap: false,
            },
            image_bounds,
            mag_bounds,
        );

        let (mag_center_x, mag_center_y) = (center.x, center.y);
        // Quads in a layer are drawn before its images, so the ring and crosshair
        // go in a layer of their own to land on top of the loupe.
        renderer.with_layer(mag_bounds, |renderer| {
            // Crosshair spanning the loupe, up to the inside of the ring.
            let crosshair_size = MAGNIFIER_RADIUS - 2.0;
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
                    snap: false,
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
                    snap: false,
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
                    snap: false,
                },
                Color::TRANSPARENT,
            );
        });
    });
}

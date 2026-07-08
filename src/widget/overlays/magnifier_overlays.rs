//! Magnifier annotation overlay drawing functions
//!
//! Draws magnifier loupes onto the screenshot preview: a circular region that
//! shows the underlying image content zoomed in, plus a ring outline.
//!
//! The zoomed content is drawn as a single scaled image primitive (clipped to a
//! circle via `border_radius`), which is both fast and seamless — far better
//! than filling the circle with one quad per source pixel.

use cosmic::iced::Radians;
use cosmic::iced::Color;
use cosmic::iced::advanced::image::{FilterMethod, Handle, Image, Renderer as ImageRenderer};
use cosmic::iced::core::{Background, Border, Rectangle, Renderer as _, Shadow, renderer::Quad};

use crate::config::ShapeColor;
use crate::domain::Annotation;
use crate::render::geometry::{self, shape};

/// Draw all magnifier annotations from the unified annotations array
#[allow(clippy::too_many_arguments)]
pub fn draw_magnifiers(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    annotations: &[Annotation],
    output_offset: (f32, f32),
    handle: &Handle,
    base_rect: Rectangle,
) {
    for annotation in annotations {
        if let Annotation::Magnifier(m) = annotation {
            let (offset_x, offset_y) = output_offset;
            draw_magnifier_circle(
                renderer,
                viewport,
                m.start_x - offset_x,
                m.start_y - offset_y,
                m.end_x - offset_x,
                m.end_y - offset_y,
                m.magnification,
                m.color,
                m.shadow,
                handle,
                base_rect,
            );
        }
    }
}

/// Draw a magnifier preview while dragging
#[allow(clippy::too_many_arguments)]
pub fn draw_magnifier_preview(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    start: (f32, f32), // Global start position
    end: (f32, f32),   // Local end position (cursor)
    output_offset: (f32, f32),
    magnification: f32,
    color: ShapeColor,
    shadow: bool,
    handle: &Handle,
    base_rect: Rectangle,
) {
    let (offset_x, offset_y) = output_offset;
    draw_magnifier_circle(
        renderer,
        viewport,
        start.0 - offset_x,
        start.1 - offset_y,
        end.0,
        end.1,
        magnification,
        color,
        shadow,
        handle,
        base_rect,
    );
}

/// Draw selection handles (an accent ring + 4 cardinal grab handles) around a
/// magnifier, given its center/radius in output-local logical coordinates.
pub fn draw_magnifier_handles(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    cx: f32,
    cy: f32,
    radius: f32,
    accent: Color,
) {
    let ring_bounds = Rectangle {
        x: cx - radius,
        y: cy - radius,
        width: radius * 2.0,
        height: radius * 2.0,
    };
    let handle_r = 6.0_f32;
    let handles = [
        (cx, cy - radius),
        (cx, cy + radius),
        (cx - radius, cy),
        (cx + radius, cy),
    ];

    renderer.with_layer(*viewport, |renderer| {
        // Accent highlight ring
        renderer.fill_quad(
            Quad {
                bounds: ring_bounds,
                border: Border {
                    radius: radius.into(),
                    width: 2.0,
                    color: accent,
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Background::Color(Color::TRANSPARENT),
        );

        for (hx, hy) in handles {
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: hx - handle_r,
                        y: hy - handle_r,
                        width: handle_r * 2.0,
                        height: handle_r * 2.0,
                    },
                    border: Border {
                        radius: handle_r.into(),
                        width: 1.5,
                        color: Color::WHITE,
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                Background::Color(accent),
            );
        }
    });
}

// ============ Internal helpers ============

/// Draw a single magnifier circle given local (output-relative) logical points
#[allow(clippy::too_many_arguments)]
fn draw_magnifier_circle(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    magnification: f32,
    color: ShapeColor,
    shadow: bool,
    handle: &Handle,
    base_rect: Rectangle,
) {
    let (cx, cy, radius) = geometry::circle_from_points(x1, y1, x2, y2);
    if radius < 2.0 {
        return;
    }
    let mag = magnification.max(1.0);

    // Scale the background image (which is drawn to fill `base_rect`) by `mag`
    // around the magnifier center, so the content at (cx, cy) stays fixed while
    // everything around it is zoomed. Anchoring to `base_rect` (rather than
    // assuming a 0-origin, output-sized rect) keeps the center aligned under
    // fractional display scaling.
    let scaled_bounds = Rectangle {
        x: cx - (cx - base_rect.x) * mag,
        y: cy - (cy - base_rect.y) * mag,
        width: base_rect.width * mag,
        height: base_rect.height * mag,
    };
    let clip_bounds = Rectangle {
        x: cx - radius,
        y: cy - radius,
        width: radius * 2.0,
        height: radius * 2.0,
    };

    let ring_bounds = clip_bounds;

    // Zoomed content: a single scaled image clipped to a circle.
    renderer.with_layer(*viewport, |renderer| {
        let image = Image {
            handle: handle.clone(),
            filter_method: FilterMethod::Linear,
            rotation: Radians(0.0),
            border_radius: radius.into(),
            opacity: 1.0,
            snap: false,
        };
        renderer.draw_image(image, scaled_bounds, clip_bounds);
    });

    // The ring must be drawn in a SEPARATE, later layer: within a single layer the
    // renderer composites images on top of quads regardless of call order, which
    // would otherwise hide the ring beneath the zoomed image.
    renderer.with_layer(*viewport, |renderer| {
        // Shadow ring
        if shadow {
            renderer.fill_quad(
                Quad {
                    bounds: ring_bounds,
                    border: Border {
                        radius: radius.into(),
                        width: shape::BORDER_THICKNESS,
                        color: Color::from_rgba(0.0, 0.0, 0.0, 0.9),
                    },
                    shadow: Shadow::default(),
                    snap: false,
                },
                Background::Color(Color::TRANSPARENT),
            );
        }

        // Colored ring on top
        renderer.fill_quad(
            Quad {
                bounds: ring_bounds,
                border: Border {
                    radius: radius.into(),
                    width: shape::THICKNESS,
                    color: color.into(),
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Background::Color(Color::TRANSPARENT),
        );
    });
}

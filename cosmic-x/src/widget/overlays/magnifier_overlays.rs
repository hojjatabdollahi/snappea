// SPDX-License-Identifier: GPL-3.0-only

//! Selection handles for the magnifier being adjusted.

use cosmic::iced::Color;
use cosmic::iced::core::{Background, Border, Rectangle, Renderer as _, Shadow, renderer::Quad};

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

// SPDX-License-Identifier: GPL-3.0-only

use cosmic::iced::Color;
use cosmic::iced::core::{Background, Border, Rectangle, Renderer as _, Shadow, renderer::Quad};

pub fn draw_magnifier_handles(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    cx: f32,
    cy: f32,
    radius: f32,
    accent: Color,
) {
    let handle_r = 6.0_f32;
    let reach = radius + super::shapes_overlay::SELECTION_PAD;
    let handles = [(cx - reach, cy - reach), (cx + reach, cy + reach)];

    renderer.with_layer(*viewport, |renderer| {
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

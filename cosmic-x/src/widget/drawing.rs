// SPDX-License-Identifier: GPL-3.0-only

//! Drawing helpers shared by the selection widgets.

use cosmic::iced::core::{
    Background, Border, Color, Point, Rectangle, Renderer as _, Shadow, Size, renderer::Quad,
};

/// Darken an inactive output and center a hint on it.
pub fn draw_inactive_overlay_with_hint(
    renderer: &mut cosmic::Renderer,
    bounds: Rectangle,
    hint_text: &str,
    overlay_opacity: f32,
) {
    use cosmic::iced::core::text::{Renderer as TextRenderer, Text};

    // Draw dark overlay
    let dark_overlay = Color::from_rgba(0.0, 0.0, 0.0, overlay_opacity);
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
        },
        Background::Color(dark_overlay),
    );

    // Draw a centered hint box with text
    let font_size = 18.0;
    let box_width = 420.0_f32;
    let box_height = 50.0_f32;

    // Center the box in the screen
    let box_x = bounds.x + (bounds.width - box_width) / 2.0;
    let box_y = bounds.y + (bounds.height - box_height) / 2.0;

    let hint_box = Rectangle {
        x: box_x,
        y: box_y,
        width: box_width,
        height: box_height,
    };

    // Draw semi-transparent background for the hint box
    renderer.fill_quad(
        Quad {
            bounds: hint_box,
            border: Border {
                radius: 8.0.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            shadow: Shadow::default(),
            snap: false,
        },
        Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5)),
    );

    // Draw text centered in the hint box
    renderer.fill_text(
        Text {
            content: hint_text.to_string(),
            bounds: Size::new(box_width, box_height),
            size: cosmic::iced::core::Pixels(font_size),
            line_height: cosmic::iced::core::text::LineHeight::Relative(1.0),
            font: cosmic::iced::core::Font {
                weight: cosmic::iced::core::font::Weight::Medium,
                ..Default::default()
            },
            align_x: cosmic::iced::alignment::Horizontal::Center.into(),
            align_y: cosmic::iced::alignment::Vertical::Center,
            shaping: cosmic::iced::core::text::Shaping::Advanced,
            wrapping: cosmic::iced::core::text::Wrapping::None,
            ellipsize: cosmic::iced::core::text::Ellipsize::default(),
        },
        Point::new(box_x + box_width / 2.0, box_y + box_height / 2.0),
        Color::WHITE,
        hint_box,
    );
}

/// Draw a 2px accent frame with circular corner handles. Skipped for a full-screen selection.
pub fn draw_selection_frame_with_handles(
    renderer: &mut cosmic::Renderer,
    selection_rect: (f32, f32, f32, f32),
    output_size: (f32, f32),
    accent_color: Color,
    corner_radius: f32,
) {
    let (sel_x, sel_y, sel_w, sel_h) = selection_rect;
    let (output_width, output_height) = output_size;

    // Skip if selection is too small
    if sel_w <= 0.0 || sel_h <= 0.0 {
        return;
    }

    // Skip if selection covers the entire output (screen mode)
    let is_full_screen = sel_x == 0.0
        && sel_y == 0.0
        && (sel_w - output_width).abs() < 1.0
        && (sel_h - output_height).abs() < 1.0;

    if is_full_screen {
        return;
    }

    // Selection border (2px accent color)
    let sel_rect = Rectangle {
        x: sel_x,
        y: sel_y,
        width: sel_w,
        height: sel_h,
    };
    renderer.fill_quad(
        Quad {
            bounds: sel_rect,
            border: Border {
                radius: 0.0.into(),
                width: 2.0,
                color: accent_color,
            },
            shadow: Shadow::default(),
            snap: false,
        },
        Background::Color(Color::TRANSPARENT),
    );

    // Corner handles (circles at each corner)
    let corner_size = 12.0_f32;
    let corners = [
        (sel_x, sel_y),                 // NW
        (sel_x + sel_w, sel_y),         // NE
        (sel_x, sel_y + sel_h),         // SW
        (sel_x + sel_w, sel_y + sel_h), // SE
    ];
    for (cx, cy) in corners {
        let bounds = Rectangle {
            x: cx - corner_size / 2.0,
            y: cy - corner_size / 2.0,
            width: corner_size,
            height: corner_size,
        };
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    radius: corner_radius.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                shadow: Shadow::default(),
                snap: false,
            },
            Background::Color(accent_color),
        );
    }
}

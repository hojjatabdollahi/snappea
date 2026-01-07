//! Shared drawing utilities for widget rendering
//!
//! This module contains reusable drawing functions used across multiple widgets.

use cosmic::iced_core::{
    Background, Border, Color, Point, Rectangle, Renderer as _, Shadow, Size, renderer::Quad,
};

/// Draw a dark overlay around a selection rectangle
///
/// This draws 4 rectangular strips (top, bottom, left, right) around the
/// `selection_rect` within `outer_bounds`, creating a "spotlight" effect.
///
/// # Arguments
/// * `renderer` - The renderer to draw with
/// * `outer_bounds` - The full bounds to cover with the overlay
/// * `selection_rect` - The rectangle to leave clear (in same coordinate space as outer_bounds)
/// * `opacity` - Opacity of the dark overlay (0.0 = transparent, 1.0 = opaque)
pub fn draw_dark_overlay_around_selection(
    renderer: &mut cosmic::Renderer,
    outer_bounds: Rectangle,
    selection_rect: Rectangle,
    opacity: f32,
) {
    let overlay = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: opacity,
    };

    let sel = selection_rect;
    let outer = outer_bounds;

    // Top strip
    if sel.y > outer.y {
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(outer.x, outer.y),
                    Size::new(outer.width, sel.y - outer.y),
                ),
                border: Border::default(),
                shadow: Shadow::default(),
            },
            overlay,
        );
    }

    // Bottom strip
    let sel_bottom = sel.y + sel.height;
    let outer_bottom = outer.y + outer.height;
    if sel_bottom < outer_bottom {
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(outer.x, sel_bottom),
                    Size::new(outer.width, outer_bottom - sel_bottom),
                ),
                border: Border::default(),
                shadow: Shadow::default(),
            },
            overlay,
        );
    }

    // Left strip (between top and bottom)
    if sel.x > outer.x {
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(outer.x, sel.y),
                    Size::new(sel.x - outer.x, sel.height),
                ),
                border: Border::default(),
                shadow: Shadow::default(),
            },
            overlay,
        );
    }

    // Right strip (between top and bottom)
    let sel_right = sel.x + sel.width;
    let outer_right = outer.x + outer.width;
    if sel_right < outer_right {
        renderer.fill_quad(
            Quad {
                bounds: Rectangle::new(
                    Point::new(sel_right, sel.y),
                    Size::new(outer_right - sel_right, sel.height),
                ),
                border: Border::default(),
                shadow: Shadow::default(),
            },
            overlay,
        );
    }
}

/// Draw a full-screen dark overlay with a centered hint box
///
/// Used to indicate non-active outputs in multi-monitor setups.
pub fn draw_inactive_overlay_with_hint(
    renderer: &mut cosmic::Renderer,
    bounds: Rectangle,
    hint_text: &str,
    overlay_opacity: f32,
) {
    use cosmic::iced_core::text::{Renderer as TextRenderer, Text};

    // Draw dark overlay
    let dark_overlay = Color::from_rgba(0.0, 0.0, 0.0, overlay_opacity);
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border::default(),
            shadow: Shadow::default(),
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
        },
        Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5)),
    );

    // Draw text centered in the hint box
    renderer.fill_text(
        Text {
            content: hint_text.to_string(),
            bounds: Size::new(box_width, box_height),
            size: cosmic::iced_core::Pixels(font_size),
            line_height: cosmic::iced_core::text::LineHeight::Relative(1.0),
            font: cosmic::iced_core::Font {
                weight: cosmic::iced_core::font::Weight::Medium,
                ..Default::default()
            },
            horizontal_alignment: cosmic::iced::alignment::Horizontal::Center,
            vertical_alignment: cosmic::iced::alignment::Vertical::Center,
            shaping: cosmic::iced_core::text::Shaping::Advanced,
            wrapping: cosmic::iced_core::text::Wrapping::None,
        },
        Point::new(box_x + box_width / 2.0, box_y + box_height / 2.0),
        Color::WHITE,
        hint_box,
    );
}

/// Draw a selection frame with corner handles
///
/// Draws a 2px accent border around the selection rectangle and
/// filled circular handles at each corner.
///
/// # Arguments
/// * `renderer` - The renderer to draw with
/// * `selection_rect` - The selection rectangle (x, y, w, h)
/// * `output_size` - The output size (width, height) to check for full-screen
/// * `accent_color` - The accent color for border and handles
/// * `corner_radius` - Corner radius for the handles
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
            },
            Background::Color(accent_color),
        );
    }
}

//! Status overlay drawing functions
//!
//! This module contains helper functions for drawing various status overlays:
//! - QR scanning status indicator
//! - Detected QR code overlays with labels
//! - OCR status indicator  
//! - OCR text region overlays

use cosmic::iced::{Color, Point, Size};
use cosmic::iced_core::{
    Background, Border, Rectangle, alignment,
    text::{Renderer as TextRenderer, Text},
};

use crate::capture::ocr::OcrStatus;

/// Check if a string looks like a URL
pub fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www.")
}

/// Draw a status indicator badge with text
pub fn draw_status_badge(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    text: &str,
    x: f32,
    y: f32,
    border_color: Color,
    corner_radius: f32,
) {
    let font_size = 16.0_f32;
    let char_width = font_size * 0.55;
    let text_width = text.len() as f32 * char_width;
    let text_height = font_size * 1.4;
    let padding_h = 16.0;
    let padding_v = 10.0;

    let bg_width = text_width + padding_h * 2.0;
    let bg_height = text_height + padding_v * 2.0;

    let bg_rect = Rectangle {
        x,
        y,
        width: bg_width,
        height: bg_height,
    };

    use cosmic::iced_core::Renderer as RendererTrait;
    renderer.with_layer(*viewport, |renderer| {
        renderer.fill_quad(
            cosmic::iced_core::renderer::Quad {
                bounds: bg_rect,
                border: Border {
                    radius: corner_radius.into(),
                    width: 2.0,
                    color: border_color,
                },
                shadow: cosmic::iced_core::Shadow::default(),
            },
            Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.80)),
        );

        let text_content = Text {
            content: text.to_string(),
            bounds: Size::new(bg_width, bg_height),
            size: cosmic::iced::Pixels(font_size),
            line_height: cosmic::iced_core::text::LineHeight::default(),
            font: cosmic::iced::Font::default(),
            horizontal_alignment: alignment::Horizontal::Center,
            vertical_alignment: alignment::Vertical::Center,
            shaping: cosmic::iced_core::text::Shaping::Advanced,
            wrapping: cosmic::iced_core::text::Wrapping::None,
        };

        renderer.fill_text(
            text_content,
            Point::new(bg_rect.x + bg_width / 2.0, bg_rect.y + bg_height / 2.0),
            Color::WHITE,
            *viewport,
        );
    });
}

/// Draw QR scanning status indicator
pub fn draw_qr_scanning_indicator(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    accent_color: Color,
    corner_radius: f32,
) {
    draw_status_badge(
        renderer,
        viewport,
        "Scanning for QR codes...",
        20.0,
        20.0,
        accent_color,
        corner_radius,
    );
}

/// Draw OCR status indicator
pub fn draw_ocr_status_indicator(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    ocr_status: &OcrStatus,
    qr_scanning: bool,
    accent_color: Color,
    corner_radius: f32,
) {
    let show_ocr_status = matches!(
        ocr_status,
        OcrStatus::DownloadingModels | OcrStatus::Running | OcrStatus::Error(_)
    );

    if !show_ocr_status {
        return;
    }

    let status_text = match ocr_status {
        OcrStatus::DownloadingModels => "Downloading OCR models...".to_string(),
        OcrStatus::Running => "Running OCR...".to_string(),
        OcrStatus::Error(err) => format!(
            "OCR error: {}",
            if err.len() > 40 {
                format!("{}...", &err[..37])
            } else {
                err.clone()
            }
        ),
        _ => return,
    };

    // Position below QR scanning indicator if it's showing
    let y_offset = if qr_scanning { 60.0 } else { 20.0 };

    let border_color = match ocr_status {
        OcrStatus::Error(_) => Color::from_rgb(0.9, 0.2, 0.2), // Red
        _ => accent_color,
    };

    draw_status_badge(
        renderer,
        viewport,
        &status_text,
        20.0,
        y_offset,
        border_color,
        corner_radius,
    );
}

/// Draw detected QR code overlays
#[allow(clippy::too_many_arguments)]
pub fn draw_qr_code_overlays(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    qr_codes: &[(f32, f32, String)],
    selection_rect: (f32, f32, f32, f32), // (x, y, w, h)
    accent_color: Color,
    corner_radius: f32,
) {
    let (sel_x, sel_y, sel_w, sel_h) = selection_rect;
    let button_size = 28.0_f32;

    use cosmic::iced_core::Renderer as RendererTrait;

    for (x, y, content) in qr_codes {
        let font_size = 14.0_f32;
        let padding = 8.0;
        let content_is_url = is_url(content);

        // Calculate max label width based on selection rectangle
        let button_space = if content_is_url {
            button_size + padding
        } else {
            0.0
        };
        let max_label_width = (sel_w - padding * 4.0 - button_space).clamp(80.0, 400.0);

        // Estimate number of lines for wrapped text
        let chars_per_line = (max_label_width / (font_size * 0.55)).max(10.0) as usize;
        let num_lines = ((content.len() / chars_per_line).max(1) + 1).min(6);
        let text_height = (num_lines as f32 * font_size * 1.3).min(sel_h * 0.6);

        let bg_width = max_label_width + padding * 2.0 + button_space;
        let bg_height = text_height.max(button_size) + padding * 2.0;

        // Position centered on QR location, but clamp to selection bounds
        let mut label_x = *x - bg_width / 2.0;
        let mut label_y = *y - bg_height / 2.0;

        label_x = label_x
            .max(sel_x + padding)
            .min(sel_x + sel_w - bg_width - padding);
        label_y = label_y
            .max(sel_y + padding)
            .min(sel_y + sel_h - bg_height - padding);

        let bg_rect = Rectangle {
            x: label_x,
            y: label_y,
            width: bg_width,
            height: bg_height,
        };

        renderer.with_layer(*viewport, |renderer| {
            // Draw background
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: bg_rect,
                    border: Border {
                        radius: corner_radius.into(),
                        width: 2.0,
                        color: accent_color,
                    },
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.80)),
            );

            // Draw text with word wrapping
            let text = Text {
                content: content.clone(),
                bounds: Size::new(max_label_width, text_height),
                size: cosmic::iced::Pixels(font_size),
                line_height: cosmic::iced_core::text::LineHeight::Relative(1.3),
                font: cosmic::iced::Font::default(),
                horizontal_alignment: alignment::Horizontal::Left,
                vertical_alignment: alignment::Vertical::Top,
                shaping: cosmic::iced_core::text::Shaping::Advanced,
                wrapping: cosmic::iced_core::text::Wrapping::Word,
            };

            renderer.fill_text(
                text,
                Point::new(bg_rect.x + padding, bg_rect.y + padding),
                Color::WHITE,
                *viewport,
            );

            // Draw "open URL" button if content is a URL
            if content_is_url {
                let button_x = bg_rect.x + bg_width - padding - button_size;
                let button_y = bg_rect.y + (bg_height - button_size) / 2.0;

                let button_rect = Rectangle {
                    x: button_x,
                    y: button_y,
                    width: button_size,
                    height: button_size,
                };

                renderer.fill_quad(
                    cosmic::iced_core::renderer::Quad {
                        bounds: button_rect,
                        border: Border {
                            radius: (button_size / 4.0).into(),
                            width: 1.0,
                            color: accent_color,
                        },
                        shadow: cosmic::iced_core::Shadow::default(),
                    },
                    Background::Color(accent_color),
                );

                let icon_text = Text {
                    content: "🔗".to_string(),
                    bounds: Size::new(button_size, button_size),
                    size: cosmic::iced::Pixels(16.0),
                    line_height: cosmic::iced_core::text::LineHeight::default(),
                    font: cosmic::iced::Font::default(),
                    horizontal_alignment: alignment::Horizontal::Center,
                    vertical_alignment: alignment::Vertical::Center,
                    shaping: cosmic::iced_core::text::Shaping::Advanced,
                    wrapping: cosmic::iced_core::text::Wrapping::None,
                };

                renderer.fill_text(
                    icon_text,
                    Point::new(button_x + button_size / 2.0, button_y + button_size / 2.0),
                    Color::WHITE,
                    *viewport,
                );
            }
        });
    }
}

/// OCR block color palette
const OCR_BLOCK_COLORS: [Color; 8] = [
    Color::from_rgb(0.2, 0.6, 0.9), // Blue
    Color::from_rgb(0.9, 0.3, 0.3), // Red
    Color::from_rgb(0.3, 0.8, 0.3), // Green
    Color::from_rgb(0.9, 0.6, 0.2), // Orange
    Color::from_rgb(0.7, 0.3, 0.9), // Purple
    Color::from_rgb(0.2, 0.8, 0.8), // Cyan
    Color::from_rgb(0.9, 0.9, 0.2), // Yellow
    Color::from_rgb(0.9, 0.4, 0.7), // Pink
];

/// Draw OCR text region overlays
pub fn draw_ocr_overlays(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    ocr_overlays: &[(f32, f32, f32, f32, i32)], // (left, top, width, height, block_num)
) {
    use cosmic::iced_core::Renderer as RendererTrait;

    for (left, top, width, height, block_num) in ocr_overlays {
        let color_idx = (*block_num as usize) % OCR_BLOCK_COLORS.len();
        let border_color = OCR_BLOCK_COLORS[color_idx];

        let rect = Rectangle {
            x: *left,
            y: *top,
            width: *width,
            height: *height,
        };

        renderer.with_layer(*viewport, |renderer| {
            renderer.fill_quad(
                cosmic::iced_core::renderer::Quad {
                    bounds: rect,
                    border: Border {
                        radius: 2.0.into(),
                        width: 2.0,
                        color: border_color,
                    },
                    shadow: cosmic::iced_core::Shadow::default(),
                },
                Background::Color(Color::TRANSPARENT),
            );
        });
    }
}

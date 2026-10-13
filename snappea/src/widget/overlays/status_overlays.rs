// SPDX-License-Identifier: GPL-3.0-only

use crate::capture::detect::OcrStatus;
use crate::capture::detect::{DetectedQrCode, OcrTextOverlay, QrAction, actions_for};
use crate::fl;
use cosmic::iced::Color;
use cosmic::iced::Point;
use cosmic::iced::Size;
use cosmic::iced::core::Background;
use cosmic::iced::core::Border;
use cosmic::iced::core::Rectangle;
use cosmic::iced::core::alignment;
use cosmic::iced::core::text::Renderer as TextRenderer;
use cosmic::iced::core::text::Text;

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

    use cosmic::iced::core::Renderer as RendererTrait;
    renderer.with_layer(*viewport, |renderer| {
        renderer.fill_quad(
            cosmic::iced::core::renderer::Quad {
                bounds: bg_rect,
                border: Border {
                    radius: corner_radius.into(),
                    width: 2.0,
                    color: border_color,
                },
                shadow: cosmic::iced::core::Shadow::default(),
                snap: false,
            },
            Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.80)),
        );

        let text_content = Text {
            content: text.to_string(),
            bounds: Size::new(bg_width, bg_height),
            size: cosmic::iced::Pixels(font_size),
            line_height: cosmic::iced::core::text::LineHeight::default(),
            font: cosmic::iced::Font::default(),
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            shaping: cosmic::iced::core::text::Shaping::Advanced,
            wrapping: cosmic::iced::core::text::Wrapping::None,
            ellipsize: cosmic::iced::core::text::Ellipsize::default(),
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
        &fl!("scanning-qr"),
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
    let status_text = match ocr_status {
        OcrStatus::Running => fl!("running-ocr"),
        OcrStatus::Error(err) => {
            let truncated = if err.chars().count() > 40 {
                format!("{}...", err.chars().take(37).collect::<String>())
            } else {
                err.clone()
            };
            fl!("ocr-error", error = truncated)
        }
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

/// Stroke a dashed rectangle as runs of short quads.
fn stroke_dashed_rect(
    renderer: &mut cosmic::Renderer,
    rect: Rectangle,
    color: Color,
    thickness: f32,
) {
    use cosmic::iced::core::Renderer as RendererTrait;

    const DASH: f32 = 5.0;
    const GAP: f32 = 4.0;
    let step = DASH + GAP;

    let mut dash = |x: f32, y: f32, w: f32, h: f32| {
        renderer.fill_quad(
            cosmic::iced::core::renderer::Quad {
                bounds: Rectangle::new((x, y).into(), (w, h).into()),
                border: Border::default(),
                shadow: cosmic::iced::core::Shadow::default(),
                snap: false,
            },
            Background::Color(color),
        );
    };

    let mut x = rect.x;
    while x < rect.x + rect.width {
        let w = DASH.min(rect.x + rect.width - x);
        dash(x, rect.y, w, thickness);
        dash(x, rect.y + rect.height - thickness, w, thickness);
        x += step;
    }

    let mut y = rect.y;
    while y < rect.y + rect.height {
        let h = DASH.min(rect.y + rect.height - y);
        dash(rect.x, y, thickness, h);
        dash(rect.x + rect.width - thickness, y, thickness, h);
        y += step;
    }
}

/// Draw the badge under each detected QR code, laid out by `layout_badge` like the hit testing.
pub fn draw_qr_code_overlays(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    qr_codes: &[&DetectedQrCode],
    selection_rect: (f32, f32, f32, f32),
    accent_color: Color,
) {
    use cosmic::iced::core::Renderer as RendererTrait;
    use cosmic::iced::core::text::Renderer as _;

    let (sel_x, sel_y, sel_w, sel_h) = selection_rect;
    let clamp = Rectangle::new((sel_x, sel_y).into(), (sel_w, sel_h).into());

    for qr in qr_codes {
        let badge = layout_badge(
            (qr.center_x, qr.center_y),
            qr.half_height,
            &qr.content,
            clamp,
        );

        renderer.with_layer(*viewport, |renderer| {
            // Outline what was read, so it is clear which code the badge
            // belongs to when there is more than one.
            const OUTLINE_PAD: f32 = 4.0;
            stroke_dashed_rect(
                renderer,
                Rectangle::new(
                    (
                        qr.center_x - qr.half_width - OUTLINE_PAD,
                        qr.center_y - qr.half_height - OUTLINE_PAD,
                    )
                        .into(),
                    (
                        (qr.half_width + OUTLINE_PAD) * 2.0,
                        (qr.half_height + OUTLINE_PAD) * 2.0,
                    )
                        .into(),
                ),
                accent_color,
                1.0,
            );

            renderer.fill_quad(
                cosmic::iced::core::renderer::Quad {
                    bounds: badge.bounds,
                    border: Border {
                        radius: (badge.bounds.height / 2.0).into(),
                        ..Default::default()
                    },
                    shadow: cosmic::iced::core::Shadow::default(),
                    snap: false,
                },
                Background::Color(accent_color),
            );

            // Dark on the accent fill, as the design has it.
            let on_accent = Color::from_rgb(0.1, 0.1, 0.1);

            renderer.fill_text(
                Text {
                    content: qr.content.clone(),
                    bounds: badge.label.size(),
                    size: badge.label_size.into(),
                    font: cosmic::font::default(),
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Top,
                    shaping: cosmic::iced::core::text::Shaping::Advanced,
                    wrapping: cosmic::iced::core::text::Wrapping::None,
                    line_height: cosmic::iced::core::text::LineHeight::default(),
                    // One line that trails off. A long payload must not stretch
                    // the badge across the screen.
                    ellipsize: cosmic::iced::core::text::Ellipsize::default(),
                },
                badge.label.position(),
                on_accent,
                badge.label,
            );

            for (action, rect) in &badge.actions {
                let icon = cosmic::widget::icon::from_name(action.icon_name())
                    .size(16)
                    .icon()
                    .into_svg_handle();
                if let Some(handle) = icon {
                    cosmic::iced::advanced::svg::Renderer::draw_svg(
                        renderer,
                        cosmic::iced::advanced::svg::Svg {
                            handle,
                            color: Some(on_accent),
                            rotation: cosmic::iced::Radians(0.0),
                            opacity: 1.0,
                            border_radius: [0.0; 4],
                        },
                        *rect,
                        *rect,
                    );
                }
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
    ocr_overlays: &[&OcrTextOverlay],
) {
    use cosmic::iced::core::Renderer as RendererTrait;

    for overlay in ocr_overlays {
        let color_idx = (overlay.block_num as usize) % OCR_BLOCK_COLORS.len();
        let border_color = OCR_BLOCK_COLORS[color_idx];

        let rect = Rectangle {
            x: overlay.left,
            y: overlay.top,
            width: overlay.width,
            height: overlay.height,
        };

        renderer.with_layer(*viewport, |renderer| {
            renderer.fill_quad(
                cosmic::iced::core::renderer::Quad {
                    bounds: rect,
                    border: Border {
                        radius: 2.0.into(),
                        width: 2.0,
                        color: border_color,
                    },
                    shadow: cosmic::iced::core::Shadow::default(),
                    snap: false,
                },
                Background::Color(Color::TRANSPARENT),
            );
        });
    }
}

/// Height of the badge, and so its corner radius.
pub const BADGE_H: f32 = 36.0;
/// Size of each action button inside the badge.
pub const ACTION_SIZE: f32 = 20.0;
/// Space inside the badge's ends and between its parts.
pub const BADGE_PAD: f32 = 10.0;
/// Gap between the code and the badge below it.
pub const BADGE_GAP: f32 = 8.0;
/// Longest label drawn before it is ellipsised, so one long payload cannot
/// stretch the badge across the screen.
pub const LABEL_MAX: f32 = 260.0;
/// Shortest the label is squeezed to before the badge is allowed to overhang.
pub const LABEL_MIN: f32 = 80.0;

/// Badge geometry, shared by drawing and hit testing.
#[derive(Debug, Clone)]
pub struct QrBadge {
    pub bounds: Rectangle,
    pub label: Rectangle,
    pub label_size: f32,
    /// Each action and the rectangle it occupies, in draw order.
    pub actions: Vec<(QrAction, Rectangle)>,
}

/// Lay the badge out beneath a code centered at `center`, nudged inside `clamp`.
#[must_use]
pub fn layout_badge(
    center: (f32, f32),
    half_height: f32,
    content: &str,
    clamp: Rectangle,
) -> QrBadge {
    let label_size = 14.0;
    // Rough advance width: enough to size the badge without measuring text,
    // which the renderer would only do later.
    let natural = (content.chars().count() as f32 * label_size * 0.55).min(LABEL_MAX);

    let actions = actions_for(content);
    let actions_w = actions.len() as f32 * (ACTION_SIZE + BADGE_PAD);
    let chrome = BADGE_PAD.mul_add(2.0, actions_w);

    // Shorten a long label to the selection, down to a floor.
    let label_w = natural.min((clamp.width - chrome).max(LABEL_MIN));
    let width = chrome + label_w;

    let mut x = center.0 - width / 2.0;
    let mut y = center.1 + half_height + BADGE_GAP;
    x = x.clamp(clamp.x, (clamp.x + clamp.width - width).max(clamp.x));
    y = y.clamp(clamp.y, (clamp.y + clamp.height - BADGE_H).max(clamp.y));

    let bounds = Rectangle::new((x, y).into(), (width, BADGE_H).into());
    let line_h = cosmic::iced::core::text::LineHeight::default()
        .to_absolute(label_size.into())
        .0;
    let label = Rectangle::new(
        (x + BADGE_PAD, y + (BADGE_H - line_h) / 2.0).into(),
        (label_w, line_h).into(),
    );

    let mut cursor = x + BADGE_PAD + label_w + BADGE_PAD;
    let placed = actions
        .into_iter()
        .map(|action| {
            let rect = Rectangle::new(
                (cursor, y + (BADGE_H - ACTION_SIZE) / 2.0).into(),
                (ACTION_SIZE, ACTION_SIZE).into(),
            );
            cursor += ACTION_SIZE + BADGE_PAD;
            (action, rect)
        })
        .collect();

    QrBadge {
        bounds,
        label,
        label_size,
        actions: placed,
    }
}

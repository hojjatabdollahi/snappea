//! Mesh building for screen preview using iced graphics
//!
//! These functions build vertex/index buffers for anti-aliased rendering on screen.

use cosmic::iced::Color;
use cosmic::iced_core::Rectangle;
use cosmic::iced_widget::graphics::{
    Mesh,
    color::{Packed, pack},
    mesh::{Indexed, Renderer as MeshRenderer, SolidVertex2D},
};

use super::geometry::{arrow, mesh as mesh_const};

use crate::domain::ArrowAnnotation;

/// Default arrow rendering parameters
pub mod arrow_params {
    pub const THICKNESS: f32 = 4.0;
    pub const HEAD_SIZE: f32 = 16.0;
    pub const OUTLINE_PX: f32 = 1.0;
    pub const BORDER_COLOR: cosmic::iced::Color = cosmic::iced::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.9,
    };
}

/// Arrow mesh result: vertices and indices for rendering
pub type ArrowMesh = (Vec<SolidVertex2D>, Vec<u32>);

/// Build an arrow mesh using lines with rounded caps (shaft line + 2 angled head lines)
///
/// Returns None if arrow is too short to render.
pub fn build_arrow_mesh(
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    color: Color,
    thickness: f32,
    head_size: f32,
) -> Option<ArrowMesh> {
    let dx = end_x - start_x;
    let dy = end_y - start_y;
    let length = (dx * dx + dy * dy).sqrt();
    if length < arrow::MIN_LENGTH {
        return None;
    }

    // Direction vector is calculated by arrow::head_points

    let mut inner = color;
    inner.a = inner.a.clamp(0.0, 1.0);
    let packed_inner = pack(inner);

    let mut outer = color;
    outer.a = 0.0;
    let packed_outer = pack(outer);

    let radius = thickness / 2.0;
    let feather = mesh_const::FEATHER;

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // Draw the shaft line from start to end
    add_line_segment(
        &mut vertices,
        &mut indices,
        start_x,
        start_y,
        end_x,
        end_y,
        radius,
        feather,
        packed_inner,
        packed_outer,
    );

    // Add rounded caps at start and end
    add_circle(
        &mut vertices,
        &mut indices,
        start_x,
        start_y,
        radius,
        feather,
        packed_inner,
        packed_outer,
    );
    add_circle(
        &mut vertices,
        &mut indices,
        end_x,
        end_y,
        radius,
        feather,
        packed_inner,
        packed_outer,
    );

    // Arrowhead: two angled lines at the tip
    let (head1_x, head1_y, head2_x, head2_y) =
        arrow::head_points(start_x, start_y, end_x, end_y, head_size)?;

    // First head line
    add_line_segment(
        &mut vertices,
        &mut indices,
        end_x,
        end_y,
        head1_x,
        head1_y,
        radius,
        feather,
        packed_inner,
        packed_outer,
    );
    add_circle(
        &mut vertices,
        &mut indices,
        head1_x,
        head1_y,
        radius,
        feather,
        packed_inner,
        packed_outer,
    );

    // Second head line
    add_line_segment(
        &mut vertices,
        &mut indices,
        end_x,
        end_y,
        head2_x,
        head2_y,
        radius,
        feather,
        packed_inner,
        packed_outer,
    );
    add_circle(
        &mut vertices,
        &mut indices,
        head2_x,
        head2_y,
        radius,
        feather,
        packed_inner,
        packed_outer,
    );

    Some((vertices, indices))
}

/// Build a line segment with anti-aliased feathering
#[allow(clippy::too_many_arguments)]
fn add_line_segment(
    vertices: &mut Vec<SolidVertex2D>,
    indices: &mut Vec<u32>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
    feather: f32,
    packed_inner: Packed,
    packed_outer: Packed,
) {
    let ldx = x1 - x0;
    let ldy = y1 - y0;
    let llen = (ldx * ldx + ldy * ldy).sqrt();
    if llen < 0.1 {
        return;
    }
    let lnx = ldx / llen;
    let lny = ldy / llen;

    // Perpendicular
    let px = -lny;
    let py = lnx;

    let base_idx = vertices.len() as u32;

    // Inner quad (solid core)
    let inner_r = radius;
    let outer_r = radius + feather;

    // Add inner quad vertices
    vertices.push(SolidVertex2D {
        position: [x0 + px * inner_r, y0 + py * inner_r],
        color: packed_inner,
    });
    vertices.push(SolidVertex2D {
        position: [x0 - px * inner_r, y0 - py * inner_r],
        color: packed_inner,
    });
    vertices.push(SolidVertex2D {
        position: [x1 - px * inner_r, y1 - py * inner_r],
        color: packed_inner,
    });
    vertices.push(SolidVertex2D {
        position: [x1 + px * inner_r, y1 + py * inner_r],
        color: packed_inner,
    });

    // Add outer quad vertices (for feathering)
    vertices.push(SolidVertex2D {
        position: [x0 + px * outer_r, y0 + py * outer_r],
        color: packed_outer,
    });
    vertices.push(SolidVertex2D {
        position: [x0 - px * outer_r, y0 - py * outer_r],
        color: packed_outer,
    });
    vertices.push(SolidVertex2D {
        position: [x1 - px * outer_r, y1 - py * outer_r],
        color: packed_outer,
    });
    vertices.push(SolidVertex2D {
        position: [x1 + px * outer_r, y1 + py * outer_r],
        color: packed_outer,
    });

    // Inner quad triangles
    indices.extend_from_slice(&[base_idx, base_idx + 1, base_idx + 2]);
    indices.extend_from_slice(&[base_idx, base_idx + 2, base_idx + 3]);

    // Feather band triangles (+ side)
    indices.extend_from_slice(&[base_idx + 4, base_idx, base_idx + 3]);
    indices.extend_from_slice(&[base_idx + 4, base_idx + 3, base_idx + 7]);

    // Feather band triangles (- side)
    indices.extend_from_slice(&[base_idx + 5, base_idx + 6, base_idx + 2]);
    indices.extend_from_slice(&[base_idx + 5, base_idx + 2, base_idx + 1]);
}

/// Build a circle (rounded cap) with anti-aliased feathering
#[allow(clippy::too_many_arguments)]
fn add_circle(
    vertices: &mut Vec<SolidVertex2D>,
    indices: &mut Vec<u32>,
    cx: f32,
    cy: f32,
    radius: f32,
    feather: f32,
    packed_inner: Packed,
    packed_outer: Packed,
) {
    let base_idx = vertices.len() as u32;
    let segments = mesh_const::CIRCLE_SEGMENTS;

    // Center vertex
    vertices.push(SolidVertex2D {
        position: [cx, cy],
        color: packed_inner,
    });

    // Inner ring vertices
    for i in 0..segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
        vertices.push(SolidVertex2D {
            position: [cx + radius * angle.cos(), cy + radius * angle.sin()],
            color: packed_inner,
        });
    }

    // Outer ring vertices (for feathering)
    let outer_r = radius + feather;
    for i in 0..segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
        vertices.push(SolidVertex2D {
            position: [cx + outer_r * angle.cos(), cy + outer_r * angle.sin()],
            color: packed_outer,
        });
    }

    // Inner circle triangles (center to inner ring)
    for i in 0..segments {
        let next = (i + 1) % segments;
        indices.push(base_idx);
        indices.push(base_idx + 1 + i as u32);
        indices.push(base_idx + 1 + next as u32);
    }

    // Feather ring triangles (inner ring to outer ring)
    for i in 0..segments {
        let next = (i + 1) % segments;
        let inner_i = base_idx + 1 + i as u32;
        let inner_next = base_idx + 1 + next as u32;
        let outer_i = base_idx + 1 + segments as u32 + i as u32;
        let outer_next = base_idx + 1 + segments as u32 + next as u32;

        indices.push(inner_i);
        indices.push(outer_i);
        indices.push(outer_next);
        indices.push(inner_i);
        indices.push(outer_next);
        indices.push(inner_next);
    }
}

/// Draw all arrows with their shadows to the renderer
///
/// # Arguments
/// * `renderer` - The renderer to draw with
/// * `viewport` - The clipping viewport
/// * `arrows` - Slice of arrow annotations to draw
/// * `output_offset` - (left, top) offset to convert from global to widget-local coords
pub fn draw_arrows(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    arrows: &[ArrowAnnotation],
    output_offset: (f32, f32),
) {
    use cosmic::iced_core::Renderer as CoreRenderer;

    let (offset_x, offset_y) = output_offset;

    for arrow in arrows {
        let arrow_color: Color = arrow.color.into();

        // Convert global coordinates to widget-local
        let start_x = arrow.start_x - offset_x;
        let start_y = arrow.start_y - offset_y;
        let end_x = arrow.end_x - offset_x;
        let end_y = arrow.end_y - offset_y;

        // Border/shadow first, then main arrow
        if arrow.shadow
            && let Some((vertices, indices)) = build_arrow_mesh(
                start_x,
                start_y,
                end_x,
                end_y,
                arrow_params::BORDER_COLOR,
                arrow_params::THICKNESS + 2.0 * arrow_params::OUTLINE_PX,
                arrow_params::HEAD_SIZE + arrow_params::OUTLINE_PX,
            )
        {
            renderer.with_layer(*viewport, |renderer| {
                renderer.draw_mesh(Mesh::Solid {
                    buffers: Indexed { vertices, indices },
                    transformation: cosmic::iced_core::Transformation::IDENTITY,
                    clip_bounds: *viewport,
                });
            });
        }

        if let Some((vertices, indices)) = build_arrow_mesh(
            start_x,
            start_y,
            end_x,
            end_y,
            arrow_color,
            arrow_params::THICKNESS,
            arrow_params::HEAD_SIZE,
        ) {
            renderer.with_layer(*viewport, |renderer| {
                renderer.draw_mesh(Mesh::Solid {
                    buffers: Indexed { vertices, indices },
                    transformation: cosmic::iced_core::Transformation::IDENTITY,
                    clip_bounds: *viewport,
                });
            });
        }
    }
}

/// Draw an arrow preview (currently being drawn) with translucent colors
///
/// # Arguments
/// * `renderer` - The renderer to draw with
/// * `viewport` - The clipping viewport
/// * `start` - Start position (local coordinates)
/// * `end` - End position (local coordinates)
/// * `color` - Arrow color
/// * `with_shadow` - Whether to draw shadow/border
pub fn draw_arrow_preview(
    renderer: &mut cosmic::Renderer,
    viewport: &Rectangle,
    start: (f32, f32),
    end: (f32, f32),
    color: Color,
    with_shadow: bool,
) {
    use cosmic::iced_core::Renderer as CoreRenderer;

    let (start_x, start_y) = start;
    let (end_x, end_y) = end;

    let mut preview_color = color;
    preview_color.a = 0.7;
    let preview_border_color = Color::from_rgba(0.0, 0.0, 0.0, 0.6);

    if with_shadow
        && let Some((vertices, indices)) = build_arrow_mesh(
            start_x,
            start_y,
            end_x,
            end_y,
            preview_border_color,
            arrow_params::THICKNESS + 2.0 * arrow_params::OUTLINE_PX,
            arrow_params::HEAD_SIZE + arrow_params::OUTLINE_PX,
        )
    {
        renderer.with_layer(*viewport, |renderer| {
            renderer.draw_mesh(Mesh::Solid {
                buffers: Indexed { vertices, indices },
                transformation: cosmic::iced_core::Transformation::IDENTITY,
                clip_bounds: *viewport,
            });
        });
    }

    if let Some((vertices, indices)) = build_arrow_mesh(
        start_x,
        start_y,
        end_x,
        end_y,
        preview_color,
        arrow_params::THICKNESS,
        arrow_params::HEAD_SIZE,
    ) {
        renderer.with_layer(*viewport, |renderer| {
            renderer.draw_mesh(Mesh::Solid {
                buffers: Indexed { vertices, indices },
                transformation: cosmic::iced_core::Transformation::IDENTITY,
                clip_bounds: *viewport,
            });
        });
    }
}

//! Shared geometry calculations for annotations
//!
//! This module contains constants and math shared between
//! screen rendering (iced mesh) and image rendering (tiny-skia).

/// Arrow geometry constants
pub mod arrow {
    /// Default arrow shaft thickness in logical pixels
    pub const THICKNESS: f32 = 4.0;
    /// Default arrowhead size in logical pixels
    pub const HEAD_SIZE: f32 = 16.0;
    /// Shadow/outline thickness offset in logical pixels
    pub const OUTLINE: f32 = 2.0;
    /// Arrowhead angle from shaft in radians (35 degrees)
    pub const HEAD_ANGLE: f32 = 0.610_865_2; // 35.0_f32.to_radians()
    /// Minimum arrow length to be drawn
    pub const MIN_LENGTH: f32 = 5.0;

    /// Calculate arrow head points given start, end, and head size
    /// Returns (head1_x, head1_y, head2_x, head2_y) for the two head lines
    pub fn head_points(
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
        head_size: f32,
    ) -> Option<(f32, f32, f32, f32)> {
        let dx = end_x - start_x;
        let dy = end_y - start_y;
        let length = (dx * dx + dy * dy).sqrt();
        if length < MIN_LENGTH {
            return None;
        }

        // Unit direction vector (pointing from start to end)
        let nx = dx / length;
        let ny = dy / length;

        let cos_a = HEAD_ANGLE.cos();
        let sin_a = HEAD_ANGLE.sin();

        // First head line (rotated clockwise from arrow direction)
        let head1_dx = -nx * cos_a - (-ny) * sin_a;
        let head1_dy = -nx * sin_a + (-ny) * cos_a;
        let head1_x = end_x + head1_dx * head_size;
        let head1_y = end_y + head1_dy * head_size;

        // Second head line (rotated counter-clockwise)
        let head2_dx = -nx * cos_a + (-ny) * sin_a;
        let head2_dy = -nx * (-sin_a) + (-ny) * cos_a;
        let head2_x = end_x + head2_dx * head_size;
        let head2_y = end_y + head2_dy * head_size;

        Some((head1_x, head1_y, head2_x, head2_y))
    }
}

/// Shape (rectangle/circle) geometry constants
pub mod shape {
    /// Default stroke thickness in logical pixels
    pub const THICKNESS: f32 = 3.0;
    /// Border/shadow thickness in logical pixels
    pub const BORDER_THICKNESS: f32 = 5.0;

    /// Ellipse bezier approximation constant: 4/3 * (sqrt(2) - 1)
    pub const BEZIER_K: f32 = 0.552_284_8;
}

/// Mesh rendering constants (for anti-aliased screen preview)
pub mod mesh {
    /// Anti-aliasing feather width in pixels
    pub const FEATHER: f32 = 2.0;
    /// Number of segments for circular caps
    pub const CIRCLE_SEGMENTS: usize = 12;
}

/// Normalize min/max coordinates from arbitrary start/end points
#[inline]
pub fn normalize_rect(x1: f32, y1: f32, x2: f32, y2: f32) -> (f32, f32, f32, f32) {
    let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
    let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
    (min_x, min_y, max_x, max_y)
}

/// Calculate ellipse center and radii from bounding box
#[inline]
pub fn ellipse_from_bounds(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> (f32, f32, f32, f32) {
    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;
    let rx = ((max_x - min_x) * 0.5).max(1.0);
    let ry = ((max_y - min_y) * 0.5).max(1.0);
    (cx, cy, rx, ry)
}

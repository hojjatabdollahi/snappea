//! Annotation types for drawing on screenshots
//!
//! All annotation types store coordinates in global logical coordinates.

use crate::config::ShapeColor;

/// Arrow annotation for drawing on screenshots
#[derive(Clone, Debug, PartialEq)]
pub struct ArrowAnnotation {
    /// Start point in global logical coordinates
    pub start_x: f32,
    pub start_y: f32,
    /// End point in global logical coordinates
    pub end_x: f32,
    pub end_y: f32,
    /// Color of this arrow
    pub color: ShapeColor,
    /// Whether to draw shadow/border
    pub shadow: bool,
}

/// Redaction annotation (black rectangle) for hiding sensitive content
#[derive(Clone, Debug, PartialEq)]
pub struct RedactAnnotation {
    /// Top-left point in global logical coordinates
    pub x: f32,
    pub y: f32,
    /// Bottom-right point in global logical coordinates
    pub x2: f32,
    pub y2: f32,
}

/// Pixelation annotation for obscuring sensitive content with pixelation effect
#[derive(Clone, Debug, PartialEq)]
pub struct PixelateAnnotation {
    /// Top-left point in global logical coordinates
    pub x: f32,
    pub y: f32,
    /// Bottom-right point in global logical coordinates
    pub x2: f32,
    pub y2: f32,
    /// Block size for this pixelation
    pub block_size: u32,
}

/// Outline rectangle annotation (no fill)
#[derive(Clone, Debug, PartialEq)]
pub struct RectOutlineAnnotation {
    /// Start point in global logical coordinates
    pub start_x: f32,
    pub start_y: f32,
    /// End point in global logical coordinates
    pub end_x: f32,
    pub end_y: f32,
    /// Color of this rectangle
    pub color: ShapeColor,
    /// Whether to draw shadow/border
    pub shadow: bool,
}

/// Outline circle/ellipse annotation (no fill)
#[derive(Clone, Debug, PartialEq)]
pub struct CircleOutlineAnnotation {
    /// Start point in global logical coordinates
    pub start_x: f32,
    pub start_y: f32,
    /// End point in global logical coordinates
    pub end_x: f32,
    pub end_y: f32,
    /// Color of this circle
    pub color: ShapeColor,
    /// Whether to draw shadow/border
    pub shadow: bool,
}

/// Minimum magnifier zoom factor (matches the config slider)
pub const MAGNIFIER_MIN_ZOOM: f32 = 1.5;
/// Maximum magnifier zoom factor (matches the config slider)
pub const MAGNIFIER_MAX_ZOOM: f32 = 10.0;
/// Minimum magnifier radius in logical units
pub const MAGNIFIER_MIN_RADIUS: f32 = 12.0;

/// Magnifier annotation: a circular loupe that zooms into the content beneath it.
///
/// Defined by a bounding box (like a circle); the interior shows the underlying
/// image content scaled up by `magnification`.
#[derive(Clone, Debug, PartialEq)]
pub struct MagnifierAnnotation {
    /// Start point in global logical coordinates
    pub start_x: f32,
    pub start_y: f32,
    /// End point in global logical coordinates
    pub end_x: f32,
    pub end_y: f32,
    /// Zoom factor applied to the content under the magnifier
    pub magnification: f32,
    /// Color of the magnifier ring
    pub color: ShapeColor,
    /// Whether to draw shadow/border on the ring
    pub shadow: bool,
}

impl MagnifierAnnotation {
    /// Center point in global logical coordinates
    pub fn center(&self) -> (f32, f32) {
        ((self.start_x + self.end_x) * 0.5, (self.start_y + self.end_y) * 0.5)
    }

    /// Radius in logical units (matches `render::geometry::circle_from_points`)
    pub fn radius(&self) -> f32 {
        (((self.end_x - self.start_x).abs() + (self.end_y - self.start_y).abs()) * 0.25).max(1.0)
    }

    /// Rewrite start/end so the loupe is centered at (cx, cy) with the given radius.
    pub fn set_geometry(&mut self, cx: f32, cy: f32, radius: f32) {
        let r = radius.max(MAGNIFIER_MIN_RADIUS);
        self.start_x = cx - r;
        self.start_y = cy - r;
        self.end_x = cx + r;
        self.end_y = cy + r;
    }
}

/// Unified annotation type for ordered drawing and undo/redo
#[derive(Clone, Debug, PartialEq)]
pub enum Annotation {
    Arrow(ArrowAnnotation),
    Circle(CircleOutlineAnnotation),
    Rectangle(RectOutlineAnnotation),
    Magnifier(MagnifierAnnotation),
    Redact(RedactAnnotation),
    Pixelate(PixelateAnnotation),
}

impl Annotation {
    /// Check if this is a shape annotation (arrow, circle, rectangle, magnifier)
    pub fn is_shape(&self) -> bool {
        matches!(
            self,
            Annotation::Arrow(_)
                | Annotation::Circle(_)
                | Annotation::Rectangle(_)
                | Annotation::Magnifier(_)
        )
    }

    /// Check if this is a redaction annotation (redact, pixelate)
    pub fn is_redaction(&self) -> bool {
        matches!(self, Annotation::Redact(_) | Annotation::Pixelate(_))
    }
}

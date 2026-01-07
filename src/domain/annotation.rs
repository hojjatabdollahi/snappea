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

/// Unified annotation type for ordered drawing and undo/redo
#[derive(Clone, Debug, PartialEq)]
pub enum Annotation {
    Arrow(ArrowAnnotation),
    Circle(CircleOutlineAnnotation),
    Rectangle(RectOutlineAnnotation),
    Redact(RedactAnnotation),
    Pixelate(PixelateAnnotation),
}

impl Annotation {
    /// Check if this is a shape annotation (arrow, circle, rectangle)
    pub fn is_shape(&self) -> bool {
        matches!(
            self,
            Annotation::Arrow(_) | Annotation::Circle(_) | Annotation::Rectangle(_)
        )
    }

    /// Check if this is a redaction annotation (redact, pixelate)
    pub fn is_redaction(&self) -> bool {
        matches!(self, Annotation::Redact(_) | Annotation::Pixelate(_))
    }
}

//! Geometric types for screenshot regions and coordinates

use std::num::NonZeroU32;

/// Logical Size and Position of a rectangle
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    /// Create a new rectangle from coordinates
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Calculate the intersection of two rectangles
    pub fn intersect(&self, other: Rect) -> Option<Rect> {
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        let right = self.right.min(other.right);
        let bottom = self.bottom.min(other.bottom);
        if left < right && top < bottom {
            Some(Rect {
                left,
                top,
                right,
                bottom,
            })
        } else {
            None
        }
    }

    /// Translate the rectangle by the given offset
    pub fn translate(&self, x: i32, y: i32) -> Rect {
        Rect {
            left: self.left + x,
            top: self.top + y,
            right: self.right + x,
            bottom: self.bottom + y,
        }
    }

    /// Get the width of the rectangle
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    /// Get the height of the rectangle
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// Convert to dimensions (NonZeroU32 width and height)
    pub fn dimensions(self) -> Option<RectDimension> {
        let width = NonZeroU32::new((self.width()).unsigned_abs())?;
        let height = NonZeroU32::new((self.height()).unsigned_abs())?;
        Some(RectDimension { width, height })
    }

    /// Check if this rectangle contains a point
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// Non-zero dimensions of a rectangle
#[derive(Clone, Copy, Debug)]
pub struct RectDimension {
    pub width: NonZeroU32,
    pub height: NonZeroU32,
}

impl RectDimension {
    /// Get the width as u32
    pub fn width(&self) -> u32 {
        self.width.get()
    }

    /// Get the height as u32
    pub fn height(&self) -> u32 {
        self.height.get()
    }
}

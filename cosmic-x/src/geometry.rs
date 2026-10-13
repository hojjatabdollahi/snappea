// SPDX-License-Identifier: GPL-3.0-only

//! Integer rectangles in global logical coordinates, and what is being captured.

use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// Logical Size and Position of a rectangle
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    /// The smallest rectangle containing both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    pub fn intersect(&self, other: Self) -> Option<Self> {
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        let right = self.right.min(other.right);
        let bottom = self.bottom.min(other.bottom);
        if left < right && top < bottom {
            Some(Self {
                left,
                top,
                right,
                bottom,
            })
        } else {
            None
        }
    }

    /// Every edge multiplied by `scale` and truncated, for logical to physical.
    #[must_use]
    pub fn scaled(self, scale: f32) -> Self {
        Self {
            left: (self.left as f32 * scale) as i32,
            top: (self.top as f32 * scale) as i32,
            right: (self.right as f32 * scale) as i32,
            bottom: (self.bottom as f32 * scale) as i32,
        }
    }

    /// Translate the rectangle by the given offset
    pub const fn translate(&self, x: i32, y: i32) -> Self {
        Self {
            left: self.left + x,
            top: self.top + y,
            right: self.right + x,
            bottom: self.bottom + y,
        }
    }

    /// Get the width of the rectangle
    pub const fn width(&self) -> i32 {
        self.right - self.left
    }

    /// Get the height of the rectangle
    pub const fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// Convert to dimensions (`NonZeroU32` width and height)
    pub fn dimensions(self) -> Option<RectDimension> {
        let width = NonZeroU32::new((self.width()).unsigned_abs())?;
        let height = NonZeroU32::new((self.height()).unsigned_abs())?;
        Some(RectDimension { width, height })
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
    pub const fn width(self) -> u32 {
        self.width.get()
    }

    /// Get the height as u32
    pub const fn height(self) -> u32 {
        self.height.get()
    }
}

/// Drag state for rectangle selection handles
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragState {
    #[default]
    None,
    /// North-West corner
    NW,
    /// North edge
    N,
    /// North-East corner
    NE,
    /// East edge
    E,
    /// South-East corner
    SE,
    /// South edge
    S,
    /// South-West corner
    SW,
    /// West edge
    W,
    /// Move the entire rectangle
    Move,
}

/// Selection mode choice
#[derive(Debug, Clone)]
pub enum Choice {
    /// Output selection: None = picker mode (selecting), Some = confirmed (screen locked in)
    Output(Option<String>),
    /// Rectangle selection with current rect and drag state
    Rectangle(Rect, DragState),
    /// Every output stitched into one image. A deliberate choice, unlike an undragged `Rectangle`.
    AllScreens,
}

impl Choice {
    /// Whether something is picked out to capture.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        match self {
            Self::Rectangle(r, _) => r.dimensions().is_some(),
            // A deliberate choice of one screen or every screen is as much a
            // target as a dragged region.
            Self::Output(Some(_)) | Self::AllScreens => true,
            // Picker mode: still choosing which screen.
            Self::Output(None) => false,
        }
    }
}

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
    /// Create a new rectangle from coordinates
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Calculate the intersection of two rectangles
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

    /// Check if this rectangle contains a point
    pub const fn contains_point(&self, x: i32, y: i32) -> bool {
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
    pub const fn width(self) -> u32 {
        self.width.get()
    }

    /// Get the height as u32
    pub const fn height(self) -> u32 {
        self.height.get()
    }
}

// What is being captured and where it goes.

/// Drag state for rectangle selection handles
#[repr(u8)]
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

impl From<u8> for DragState {
    fn from(state: u8) -> Self {
        match state {
            0 => Self::None,
            1 => Self::NW,
            2 => Self::N,
            3 => Self::NE,
            4 => Self::E,
            5 => Self::SE,
            6 => Self::S,
            7 => Self::SW,
            8 => Self::W,
            9 => Self::Move,
            _ => Self::None,
        }
    }
}

impl From<DragState> for u8 {
    fn from(state: DragState) -> Self {
        match state {
            DragState::None => 0,
            DragState::NW => 1,
            DragState::N => 2,
            DragState::NE => 3,
            DragState::E => 4,
            DragState::SE => 5,
            DragState::S => 6,
            DragState::SW => 7,
            DragState::W => 8,
            DragState::Move => 9,
        }
    }
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

/// Where to save the screenshot image
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSaveLocation {
    /// Copy to clipboard only
    Clipboard,
    /// Save to Pictures folder
    #[default]
    Pictures,
    /// Save to Documents folder
    Documents,
}

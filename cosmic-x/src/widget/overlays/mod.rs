// SPDX-License-Identifier: GPL-3.0-only

//! Overlays drawn over the screenshot.

pub mod magnifier_overlays;
mod shapes_overlay;
pub mod status_overlays;

pub use shapes_overlay::{ShapesOverlay, draw_operations, samples_pixels};

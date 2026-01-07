//! Overlay widgets for screenshot selection
//!
//! This module contains canvas-based overlay widgets used for
//! rendering annotations on top of the screenshot.

pub mod redact_overlays;
mod shapes_overlay;
pub mod status_overlays;

pub use shapes_overlay::ShapesOverlay;

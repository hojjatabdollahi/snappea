//! Screenshot selection widget and related types
//!
//! This module contains the `ScreenshotSelectionWidget` which handles
//! the interactive screenshot selection UI including:
//! - Rectangle selection with drag handles
//! - Window and output selection modes  
//! - Annotation overlays (arrows, shapes, redactions)
//! - OCR and QR code detection overlays
//! - Toolbar and settings drawer
//!
//! ## Architecture
//!
//! The widget uses grouped state structs instead of individual fields:
//! - `&AnnotationState` - arrows, circles, rectangles, redactions, pixelations
//! - `&DetectionState` - QR codes, OCR overlays, scanning status
//! - `&UiState` - toolbar position, popups, settings
//!
//! Events are emitted via `ScreenshotEvent` enum and converted to `Msg`
//! by the event handler.

pub mod events;
pub mod helpers;
pub mod widget;

pub use widget::{OutputContext, ScreenshotSelectionWidget};

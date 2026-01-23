//! Screen recording functionality
//!
//! This module provides screen recording capabilities with hardware-accelerated
//! encoding via GStreamer. Recording runs in a subprocess to allow the UI to close.

mod state;
pub mod encoder;
pub mod dmabuf;
mod pipeline;
mod recorder;

pub use state::{RecordingState, is_recording, stop_recording};
pub use encoder::best_encoder;
pub use pipeline::{Pipeline, CropRegion};
pub use recorder::start_recording;

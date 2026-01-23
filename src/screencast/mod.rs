//! Screen recording functionality
//!
//! This module provides screen recording capabilities with hardware-accelerated
//! encoding via GStreamer. Recording runs in a thread to allow toplevel (window)
//! capture while keeping the UI responsive.

mod state;
pub mod encoder;
pub mod dmabuf;
mod pipeline;
mod recorder;

pub use state::{RecordingState, RecordingHandle, is_recording, stop_recording, set_recording};
pub use encoder::best_encoder;
pub use pipeline::{Pipeline, CropRegion};
pub use recorder::{start_recording, start_recording_thread};

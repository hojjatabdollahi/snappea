//! Screen recording functionality
//!
//! This module provides screen recording capabilities with hardware-accelerated
//! encoding via GStreamer. Recording runs in a thread to allow toplevel (window)
//! capture while keeping the UI responsive.

pub mod dmabuf;
pub mod encoder;
mod pipeline;
mod recorder;
mod state;

pub use encoder::best_encoder;
pub use pipeline::{CropRegion, Pipeline};
pub use recorder::{start_recording, start_recording_thread};
pub use state::{
    cancel_recording, is_recording, set_recording, stop_recording, RecordingHandle, RecordingState,
};

//! Screen recording functionality
//!
//! This module provides screen recording capabilities with hardware-accelerated
//! encoding via GStreamer. Recording runs in a subprocess to allow the UI to close.

mod state;
pub mod encoder;
mod pipeline;
mod recorder;

pub use state::{RecordingState, is_recording, stop_recording};
pub use encoder::{Codec, EncoderInfo, detect_encoders, best_encoder};
pub use pipeline::Pipeline;
pub use recorder::start_recording;

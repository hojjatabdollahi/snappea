// SPDX-License-Identifier: GPL-3.0-only

//! Screen recording through `GStreamer`, on its own thread so toplevel capture
//! does not block the UI, plus the overlay drawn while recording.

pub mod dmabuf;
pub mod encoder;
pub mod overlay;
mod pipeline;
mod recorder;
mod state;

pub use encoder::best_encoder;
pub use recorder::{record, start_recording};
pub use state::{RecordingHandle, RecordingState, is_recording, set_recording, stop_recording};

// SPDX-License-Identifier: GPL-3.0-only

pub mod dmabuf;
pub mod encoder;
pub mod overlay;
mod pipeline;
mod recorder;
mod state;

pub use encoder::best_encoder;
pub use recorder::{record, start_recording};
pub use state::{RecordingHandle, is_recording, set_recording, stop_recording};

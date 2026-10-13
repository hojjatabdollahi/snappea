// SPDX-License-Identifier: GPL-3.0-only

use crate::widgets::track_bar::COLOR_BANDS;
use std::path::Path;
use std::process::{Command, Stdio};

pub const SAMPLES: usize = 300;

/// Samples `path` into `SAMPLES * COLOR_BANDS` average colors
pub fn extract(path: &Path, duration: f64) -> Vec<[u8; 3]> {
    if duration <= 0.0 {
        return Vec::new();
    }
    let fps = SAMPLES as f64 / duration;

    let output = Command::new("ffmpeg")
        .arg("-i")
        .arg(path)
        .args([
            "-vf",
            &format!("fps={fps:.4},scale=1:{COLOR_BANDS}:flags=area"),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(o) if o.status.success() => o.stdout.as_chunks::<3>().0.to_vec(),
        _ => Vec::new(),
    }
}

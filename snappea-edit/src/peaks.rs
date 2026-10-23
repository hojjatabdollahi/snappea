// SPDX-License-Identifier: GPL-3.0-only

//! The audio lane's waveform.

use std::path::Path;
use std::process::{Command, Stdio};

pub const SAMPLES: usize = 300;

/// Samples `path`'s audio into `SAMPLES` peaks, scaled so the loudest is 1.
/// Empty when there is no audio.
pub fn extract(path: &Path) -> Vec<f32> {
    let output = Command::new("ffmpeg")
        .arg("-i")
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let samples: Vec<i16> = o
                .stdout
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect();
            reduce(&samples)
        }
        _ => Vec::new(),
    }
}

/// One peak per bucket of `samples`, scaled to the loudest bucket.
fn reduce(samples: &[i16]) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let per = samples.len().div_ceil(SAMPLES);
    let peaks: Vec<f32> = samples
        .chunks(per)
        .map(|bucket| {
            bucket
                .iter()
                .map(|s| f32::from(*s).abs())
                .fold(0.0, f32::max)
        })
        .collect();
    let loudest = peaks.iter().copied().fold(0.0, f32::max);
    if loudest <= 0.0 {
        return peaks;
    }
    peaks.iter().map(|p| p / loudest).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_scales_to_the_loudest_bucket() {
        assert_eq!(reduce(&[]), Vec::<f32>::new());
        let mut samples = vec![0i16; SAMPLES * 4];
        samples[0] = 1000;
        samples[SAMPLES * 2] = -4000;
        let peaks = reduce(&samples);
        assert_eq!(peaks.len(), SAMPLES);
        assert_eq!(peaks[0], 0.25);
        assert_eq!(peaks[SAMPLES / 2], 1.0);
        assert_eq!(peaks[1], 0.0);
        // Silence stays at zero rather than dividing by it.
        assert!(reduce(&[0, 0, 0]).iter().all(|p| *p == 0.0));
    }
}

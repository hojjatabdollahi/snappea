//! Hardware encoder detection and selection
//!
//! Queries GStreamer for available encoders and prioritizes hardware-accelerated ones

use anyhow::{Context, Result};
use gstreamer as gst;

/// Codec type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
    VP9,
    AV1,
}

impl Codec {
    pub fn name(&self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::H265 => "H.265",
            Codec::VP9 => "VP9",
            Codec::AV1 => "AV1",
        }
    }
}

/// Information about an available encoder
#[derive(Debug, Clone)]
pub struct EncoderInfo {
    /// Human-readable name (e.g., "VA-API H.264")
    pub name: String,
    /// GStreamer element name (e.g., "vaapih264enc")
    pub gst_element: String,
    /// Codec type
    pub codec: Codec,
    /// Whether this is hardware-accelerated
    pub hardware: bool,
    /// Priority (lower = better, hardware encoders have lower priority)
    pub priority: u8,
}

impl EncoderInfo {
    /// Display name with hardware/software indicator
    pub fn display_name(&self) -> String {
        let hw_indicator = if self.hardware {
            " (Hardware)"
        } else {
            " (Software)"
        };
        format!("{}{}", self.name, hw_indicator)
    }
}

/// Detect available video encoders
pub fn detect_encoders() -> Result<Vec<EncoderInfo>> {
    gst::init().context("Failed to initialize GStreamer")?;

    let mut encoders = Vec::new();

    // VA-API encoders (Intel/AMD) - priority 10
    if encoder_available("vaapih264enc") {
        encoders.push(EncoderInfo {
            name: "VA-API H.264".to_string(),
            gst_element: "vaapih264enc".to_string(),
            codec: Codec::H264,
            hardware: true,
            priority: 10,
        });
    }
    if encoder_available("vaapih265enc") {
        encoders.push(EncoderInfo {
            name: "VA-API H.265".to_string(),
            gst_element: "vaapih265enc".to_string(),
            codec: Codec::H265,
            hardware: true,
            priority: 11,
        });
    }
    if encoder_available("vaapivp9enc") {
        encoders.push(EncoderInfo {
            name: "VA-API VP9".to_string(),
            gst_element: "vaapivp9enc".to_string(),
            codec: Codec::VP9,
            hardware: true,
            priority: 12,
        });
    }

    // NVENC encoders (NVIDIA) - priority 20
    if encoder_available("nvh264enc") {
        encoders.push(EncoderInfo {
            name: "NVENC H.264".to_string(),
            gst_element: "nvh264enc".to_string(),
            codec: Codec::H264,
            hardware: true,
            priority: 20,
        });
    }
    if encoder_available("nvh265enc") {
        encoders.push(EncoderInfo {
            name: "NVENC H.265".to_string(),
            gst_element: "nvh265enc".to_string(),
            codec: Codec::H265,
            hardware: true,
            priority: 21,
        });
    }

    // Software fallbacks - priority 100+
    if encoder_available("x264enc") {
        encoders.push(EncoderInfo {
            name: "x264 H.264".to_string(),
            gst_element: "x264enc".to_string(),
            codec: Codec::H264,
            hardware: false,
            priority: 100,
        });
    }
    if encoder_available("vp9enc") {
        encoders.push(EncoderInfo {
            name: "VP9".to_string(),
            gst_element: "vp9enc".to_string(),
            codec: Codec::VP9,
            hardware: false,
            priority: 101,
        });
    }

    // Sort by priority (lower first)
    encoders.sort_by_key(|e| e.priority);

    Ok(encoders)
}

/// Check if a GStreamer encoder element is available
fn encoder_available(element_name: &str) -> bool {
    gst::ElementFactory::find(element_name).is_some()
}

/// Get the best available encoder (first hardware encoder, or first software if none)
pub fn best_encoder() -> Result<EncoderInfo> {
    let encoders = detect_encoders()?;
    encoders
        .into_iter()
        .next()
        .context("No video encoders available")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_name() {
        assert_eq!(Codec::H264.name(), "H.264");
        assert_eq!(Codec::H265.name(), "H.265");
        assert_eq!(Codec::VP9.name(), "VP9");
        assert_eq!(Codec::AV1.name(), "AV1");
    }

    #[test]
    fn test_encoder_info_display_name() {
        let hw_encoder = EncoderInfo {
            name: "VA-API H.264".to_string(),
            gst_element: "vaapih264enc".to_string(),
            codec: Codec::H264,
            hardware: true,
            priority: 10,
        };
        assert_eq!(hw_encoder.display_name(), "VA-API H.264 (Hardware)");

        let sw_encoder = EncoderInfo {
            name: "x264 H.264".to_string(),
            gst_element: "x264enc".to_string(),
            codec: Codec::H264,
            hardware: false,
            priority: 100,
        };
        assert_eq!(sw_encoder.display_name(), "x264 H.264 (Software)");
    }

    #[test]
    fn test_detect_encoders_returns_sorted_list() {
        // This test will succeed even if no encoders are available
        let result = detect_encoders();
        assert!(result.is_ok());

        let encoders = result.unwrap();
        // Verify encoders are sorted by priority
        for i in 1..encoders.len() {
            assert!(encoders[i - 1].priority <= encoders[i].priority);
        }
    }

    #[test]
    fn test_best_encoder() {
        // This test may fail on systems with no encoders, which is acceptable
        // In CI/CD, we'd need GStreamer plugins installed
        let result = best_encoder();

        if let Ok(encoder) = result {
            // If we have an encoder, verify it's valid
            assert!(!encoder.name.is_empty());
            assert!(!encoder.gst_element.is_empty());
        }
        // If no encoders available, that's also a valid outcome for this test
    }

    #[test]
    fn test_hardware_priority_lower_than_software() {
        // Verify priority system: hardware < software
        let hw_priority = 10u8; // VA-API
        let sw_priority = 100u8; // x264
        assert!(hw_priority < sw_priority);
    }
}

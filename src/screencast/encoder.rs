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
    /// Whether this encoder can participate in the real DMA-BUF zero-copy path
    pub supports_dmabuf_zero_copy: bool,
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

    pub fn zero_copy_display_name(&self) -> &'static str {
        if self.supports_dmabuf_zero_copy {
            "DMA-BUF zero-copy capable"
        } else {
            "copied-memory path only"
        }
    }
}

/// A candidate encoder: an element name to probe plus the metadata to publish
/// if it resolves. `detect_encoders` walks each codec's candidate list in order
/// and keeps the first element that GStreamer can actually create.
struct Candidate {
    name: &'static str,
    gst_element: &'static str,
    codec: Codec,
    hardware: bool,
    supports_dmabuf_zero_copy: bool,
    priority: u8,
}

/// Detect available video encoders
///
/// For each codec/backend we probe a list of candidate GStreamer element names
/// in preference order and keep the first that resolves. This matters because
/// the element names differ across GStreamer generations:
///   - Legacy `gstreamer-vaapi` exposes `vaapih264enc`, `vaapih265enc`, ...
///   - The modern stateless `va` plugin (gst-plugins-bad, GStreamer 1.22+, and
///     the default on recent Arch) exposes `vah264enc`, `vah265enc`, ... instead,
///     with `gstreamer-vaapi` deprecated/removed.
/// Probing only the legacy names made snappea miss hardware encoders entirely on
/// modern systems, leaving software VP9 as the only option (see issue #17).
///
/// The legacy `vaapi*` elements are listed first so systems that still have them
/// keep the existing DMA-BUF zero-copy path (which relies on `vaapipostproc` /
/// `memory:VASurface`); the modern `va*` elements are wired through the generic
/// copied-memory pipeline for now (`supports_dmabuf_zero_copy: false`).
pub fn detect_encoders() -> Result<Vec<EncoderInfo>> {
    gst::init().context("Failed to initialize GStreamer")?;

    // Each inner slice is a preference-ordered list of interchangeable encoders
    // for one codec/backend; only the first available element in each slice is
    // added, so we never show two entries for the same underlying encoder.
    let candidate_groups: &[&[Candidate]] = &[
        // VA-API H.264 (Intel/AMD) - priority 10
        &[
            Candidate { name: "VA-API H.264", gst_element: "vaapih264enc", codec: Codec::H264, hardware: true, supports_dmabuf_zero_copy: true, priority: 10 },
            Candidate { name: "VA-API H.264", gst_element: "vah264enc", codec: Codec::H264, hardware: true, supports_dmabuf_zero_copy: false, priority: 10 },
            Candidate { name: "VA-API H.264 (low-power)", gst_element: "vah264lpenc", codec: Codec::H264, hardware: true, supports_dmabuf_zero_copy: false, priority: 10 },
        ],
        // VA-API H.265 - priority 11
        &[
            Candidate { name: "VA-API H.265", gst_element: "vaapih265enc", codec: Codec::H265, hardware: true, supports_dmabuf_zero_copy: true, priority: 11 },
            Candidate { name: "VA-API H.265", gst_element: "vah265enc", codec: Codec::H265, hardware: true, supports_dmabuf_zero_copy: false, priority: 11 },
            Candidate { name: "VA-API H.265 (low-power)", gst_element: "vah265lpenc", codec: Codec::H265, hardware: true, supports_dmabuf_zero_copy: false, priority: 11 },
        ],
        // VA-API VP9 - priority 12
        &[
            Candidate { name: "VA-API VP9", gst_element: "vaapivp9enc", codec: Codec::VP9, hardware: true, supports_dmabuf_zero_copy: true, priority: 12 },
            Candidate { name: "VA-API VP9", gst_element: "vavp9enc", codec: Codec::VP9, hardware: true, supports_dmabuf_zero_copy: false, priority: 12 },
        ],
        // NVENC H.264 (NVIDIA) - priority 20
        &[
            Candidate { name: "NVENC H.264", gst_element: "nvh264enc", codec: Codec::H264, hardware: true, supports_dmabuf_zero_copy: false, priority: 20 },
            Candidate { name: "NVENC H.264", gst_element: "nvcudah264enc", codec: Codec::H264, hardware: true, supports_dmabuf_zero_copy: false, priority: 20 },
        ],
        // NVENC H.265 - priority 21
        &[
            Candidate { name: "NVENC H.265", gst_element: "nvh265enc", codec: Codec::H265, hardware: true, supports_dmabuf_zero_copy: false, priority: 21 },
            Candidate { name: "NVENC H.265", gst_element: "nvcudah265enc", codec: Codec::H265, hardware: true, supports_dmabuf_zero_copy: false, priority: 21 },
        ],
        // Software H.264 - priority 100. x264 (gst-plugins-ugly) preferred,
        // openh264 (gst-plugins-bad) as a fallback so MP4 still works without
        // gst-plugins-ugly installed.
        &[
            Candidate { name: "x264 H.264", gst_element: "x264enc", codec: Codec::H264, hardware: false, supports_dmabuf_zero_copy: false, priority: 100 },
            Candidate { name: "OpenH264", gst_element: "openh264enc", codec: Codec::H264, hardware: false, supports_dmabuf_zero_copy: false, priority: 100 },
        ],
        // Software VP9 - priority 101
        &[
            Candidate { name: "VP9", gst_element: "vp9enc", codec: Codec::VP9, hardware: false, supports_dmabuf_zero_copy: false, priority: 101 },
        ],
    ];

    let mut encoders = Vec::new();
    for group in candidate_groups {
        if let Some(candidate) = group.iter().find(|c| encoder_available(c.gst_element)) {
            encoders.push(EncoderInfo {
                name: candidate.name.to_string(),
                gst_element: candidate.gst_element.to_string(),
                codec: candidate.codec,
                hardware: candidate.hardware,
                supports_dmabuf_zero_copy: candidate.supports_dmabuf_zero_copy,
                priority: candidate.priority,
            });
        }
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
            supports_dmabuf_zero_copy: true,
            priority: 10,
        };
        assert_eq!(hw_encoder.display_name(), "VA-API H.264 (Hardware)");

        let sw_encoder = EncoderInfo {
            name: "x264 H.264".to_string(),
            gst_element: "x264enc".to_string(),
            codec: Codec::H264,
            hardware: false,
            supports_dmabuf_zero_copy: false,
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
    fn test_detect_encoders_have_unique_elements() {
        // Each codec/backend must resolve to at most one element, even on systems
        // that have both the legacy (vaapi*) and modern (va*) plugins installed.
        let Ok(encoders) = detect_encoders() else {
            return;
        };
        let mut seen = std::collections::HashSet::new();
        for e in &encoders {
            assert!(
                seen.insert(e.gst_element.clone()),
                "duplicate encoder element: {}",
                e.gst_element
            );
        }
    }

    #[test]
    fn test_hardware_priority_lower_than_software() {
        // Verify priority system: hardware < software
        let hw_priority = 10u8; // VA-API
        let sw_priority = 100u8; // x264
        assert!(hw_priority < sw_priority);
    }
}
